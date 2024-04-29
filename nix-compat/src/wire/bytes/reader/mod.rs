use std::{
    future::Future,
    io,
    ops::{Bound, RangeBounds},
    pin::Pin,
    task::{self, ready, Poll},
};
use tokio::io::{AsyncRead, ReadBuf};

use trailer::{read_trailer, ReadTrailer, Trailer};
mod trailer;

/// Reads a "bytes wire packet" from the underlying reader.
/// The format is the same as in [crate::wire::bytes::read_bytes],
/// however this structure provides a [AsyncRead] interface,
/// allowing to not having to pass around the entire payload in memory.
///
/// After being constructed with the underlying reader and an allowed size,
/// subsequent requests to poll_read will return payload data until the end
/// of the packet is reached.
///
/// Internally, it will first read over the size packet, filling payload_size,
/// ensuring it fits allowed_size, then return payload data.
///
/// It will not return the final bytes before all padding has been successfully
/// consumed as well, but the full length of the reader must be consumed.
///
/// In case of an error due to size constraints, or in case of not reading
/// all the way to the end (and getting a EOF), the underlying reader is no
/// longer usable and might return garbage.
pub struct BytesReader<R> {
    state: State<R>,
}

#[derive(Debug)]
enum State<R> {
    /// The data size is being read.
    Size {
        reader: Option<R>,
        /// Minimum length (inclusive)
        user_len_min: u64,
        /// Maximum length (inclusive)
        user_len_max: u64,
        filled: u8,
        buf: [u8; 8],
    },
    /// Full 8-byte blocks are being read and released to the caller.
    Body {
        reader: Option<R>,
        consumed: u64,
        /// The total length of all user data contained in both the body and trailer.
        user_len: u64,
    },
    /// The trailer is in the process of being read.
    ReadTrailer(ReadTrailer<R>),
    /// The trailer has been fully read and validated,
    /// and data can now be released to the caller.
    ReleaseTrailer { consumed: u8, data: Trailer },
}

impl<R> BytesReader<R>
where
    R: AsyncRead + Unpin,
{
    /// Constructs a new BytesReader, using the underlying passed reader.
    pub fn new<S: RangeBounds<u64>>(reader: R, allowed_size: S) -> Self {
        let user_len_min = match allowed_size.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n.saturating_add(1),
            Bound::Unbounded => 0,
        };

        let user_len_max = match allowed_size.end_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n.checked_sub(1).unwrap(),
            Bound::Unbounded => u64::MAX,
        };

        Self {
            state: State::Size {
                reader: Some(reader),
                user_len_min,
                user_len_max,
                filled: 0,
                buf: [0; 8],
            },
        }
    }

    /// Construct a new BytesReader with a known, and already-read size.
    pub fn with_size(reader: R, size: u64) -> Self {
        Self {
            state: State::Body {
                reader: Some(reader),
                consumed: 0,
                user_len: size,
            },
        }
    }

    /// Remaining data length, ie not including data already read.
    ///
    /// If the size has not been read yet, this is [None].
    #[allow(clippy::len_without_is_empty)] // if size is unknown, we can't answer that
    pub fn len(&self) -> Option<u64> {
        match self.state {
            State::Size { .. } => None,
            State::Body {
                consumed, user_len, ..
            } => Some(user_len - consumed),
            State::ReadTrailer(ref fut) => Some(fut.len() as u64),
            State::ReleaseTrailer { consumed, ref data } => {
                Some(data.len() as u64 - consumed as u64)
            }
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for BytesReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let this = &mut self.state;

        loop {
            match this {
                State::Size {
                    reader,
                    user_len_min,
                    user_len_max,
                    filled: 8,
                    buf,
                } => {
                    let reader = reader.take().unwrap();

                    let data_len = u64::from_le_bytes(*buf);
                    if data_len < *user_len_min || data_len > *user_len_max {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid size"))
                            .into();
                    }

                    *this = State::Body {
                        reader: Some(reader),
                        consumed: 0,
                        user_len: data_len,
                    };
                }
                State::Size {
                    reader,
                    filled,
                    buf,
                    ..
                } => {
                    let reader = reader.as_mut().unwrap();

                    let mut read_buf = ReadBuf::new(&mut buf[..]);
                    read_buf.advance(*filled as usize);
                    ready!(Pin::new(reader).poll_read(cx, &mut read_buf))?;

                    let new_filled = read_buf.filled().len() as u8;
                    if *filled == new_filled {
                        return Err(io::ErrorKind::UnexpectedEof.into()).into();
                    }

                    *filled = new_filled;
                }
                State::Body {
                    reader,
                    consumed,
                    user_len,
                } => {
                    let body_len = *user_len & !7;
                    let remaining = body_len - *consumed;

                    let reader = if remaining == 0 {
                        let reader = reader.take().unwrap();
                        let user_len = (*user_len & 7) as u8;
                        *this = State::ReadTrailer(read_trailer(reader, user_len));
                        continue;
                    } else {
                        reader.as_mut().unwrap()
                    };

                    let mut bytes_read = 0;
                    ready!(with_limited(buf, remaining, |buf| {
                        let ret = Pin::new(reader).poll_read(cx, buf);
                        bytes_read = buf.initialized().len();
                        ret
                    }))?;

                    *consumed += bytes_read as u64;

                    return if bytes_read != 0 {
                        Ok(())
                    } else {
                        Err(io::ErrorKind::UnexpectedEof.into())
                    }
                    .into();
                }
                State::ReadTrailer(fut) => {
                    *this = State::ReleaseTrailer {
                        consumed: 0,
                        data: ready!(Pin::new(fut).poll(cx))?,
                    };
                }
                State::ReleaseTrailer { consumed, data } => {
                    let data = &data[*consumed as usize..];
                    let data = &data[..usize::min(data.len(), buf.remaining())];

                    buf.put_slice(data);
                    *consumed += data.len() as u8;

                    return Ok(()).into();
                }
            }
        }
    }
}

/// Make a limited version of `buf`, consisting only of up to `n` bytes of the unfilled section, and call `f` with it.
/// After `f` returns, we propagate the filled cursor advancement back to `buf`.
fn with_limited<R>(buf: &mut ReadBuf, n: u64, f: impl FnOnce(&mut ReadBuf) -> R) -> R {
    let mut nbuf = buf.take(n.try_into().unwrap_or(usize::MAX));
    let ptr = nbuf.initialized().as_ptr();
    let ret = f(&mut nbuf);

    // SAFETY: `ReadBuf::take` only returns the *unfilled* section of `buf`,
    // so anything filled is new, initialized data.
    //
    // We verify that `nbuf` still points to the same buffer,
    // so we're sure it hasn't been swapped out.
    unsafe {
        // ensure our buffer hasn't been swapped out
        assert_eq!(nbuf.initialized().as_ptr(), ptr);

        let n = nbuf.filled().len();
        buf.assume_init(n);
        buf.advance(n);
    }

    ret
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::wire::bytes::{padding_len, write_bytes};
    use hex_literal::hex;
    use lazy_static::lazy_static;
    use rstest::rstest;
    use tokio::io::AsyncReadExt;
    use tokio_test::{assert_err, io::Builder};

    use super::*;

    /// The maximum length of bytes packets we're willing to accept in the test
    /// cases.
    const MAX_LEN: u64 = 1024;

    lazy_static! {
        pub static ref LARGE_PAYLOAD: Vec<u8> = (0..255).collect::<Vec<u8>>().repeat(4 * 1024);
    }

    /// Helper function, calling the (simpler) write_bytes with the payload.
    /// We use this to create data we want to read from the wire.
    async fn produce_packet_bytes(payload: &[u8]) -> Vec<u8> {
        let mut exp = vec![];
        write_bytes(&mut exp, payload).await.unwrap();
        exp
    }

    /// Read bytes packets of various length, and ensure read_to_end returns the
    /// expected payload.
    #[rstest]
    #[case::empty(&[])] // empty bytes packet
    #[case::size_1b(&[0xff])] // 1 bytes payload
    #[case::size_8b(&hex!("0001020304050607"))] // 8 bytes payload (no padding)
    #[case::size_9b(&hex!("000102030405060708"))] // 9 bytes payload (7 bytes padding)
    #[case::size_1m(LARGE_PAYLOAD.as_slice())] // larger bytes packet
    #[tokio::test]
    async fn read_payload_correct(#[case] payload: &[u8]) {
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await)
            .build();

        let mut r = BytesReader::new(&mut mock, ..=LARGE_PAYLOAD.len() as u64);
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.expect("must succeed");

        assert_eq!(payload, &buf[..]);
    }

    /// Read bytes packets of various length, and ensure read_to_end returns the
    /// expected payload.
    #[rstest]
    #[case::empty(&[])] // empty bytes packet
    #[case::size_1b(&[0xff])] // 1 bytes payload
    #[case::size_8b(&hex!("0001020304050607"))] // 8 bytes payload (no padding)
    #[case::size_9b(&hex!("000102030405060708"))] // 9 bytes payload (7 bytes padding)
    #[case::size_1m(LARGE_PAYLOAD.as_slice())] // larger bytes packet
    #[tokio::test]
    async fn read_payload_correct_known(#[case] payload: &[u8]) {
        let packet = produce_packet_bytes(payload).await;

        let size = u64::from_le_bytes({
            let mut buf = [0; 8];
            buf.copy_from_slice(&packet[..8]);
            buf
        });

        let mut mock = Builder::new().read(&packet[8..]).build();

        let mut r = BytesReader::with_size(&mut mock, size);
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.expect("must succeed");

        assert_eq!(payload, &buf[..]);
    }

    /// Fail if the bytes packet is larger than allowed
    #[tokio::test]
    async fn read_bigger_than_allowed_fail() {
        let payload = LARGE_PAYLOAD.as_slice();
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[0..8]) // We stop reading after the size packet
            .build();

        let mut r = BytesReader::new(&mut mock, ..2048);
        let mut buf = Vec::new();
        assert_err!(r.read_to_end(&mut buf).await);
    }

    /// Fail if the bytes packet is smaller than allowed
    #[tokio::test]
    async fn read_smaller_than_allowed_fail() {
        let payload = &[0x00, 0x01, 0x02];
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[0..8]) // We stop reading after the size packet
            .build();

        let mut r = BytesReader::new(&mut mock, 1024..2048);
        let mut buf = Vec::new();
        assert_err!(r.read_to_end(&mut buf).await);
    }

    /// Fail if the padding is not all zeroes
    #[tokio::test]
    async fn read_fail_if_nonzero_padding() {
        let payload = &[0x00, 0x01, 0x02];
        let mut packet_bytes = produce_packet_bytes(payload).await;
        // Flip some bits in the padding
        packet_bytes[12] = 0xff;
        let mut mock = Builder::new().read(&packet_bytes).build(); // We stop reading after the faulty bit

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);
        let mut buf = Vec::new();

        r.read_to_end(&mut buf).await.expect_err("must fail");
    }

    /// Start a 9 bytes payload packet, but have the underlying reader return
    /// EOF in the middle of the size packet (after 4 bytes).
    /// We should get an unexpected EOF error, already when trying to read the
    /// first byte (of payload)
    #[tokio::test]
    async fn read_9b_eof_during_size() {
        let payload = &hex!("FF0102030405060708");
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..4])
            .build();

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);
        let mut buf = [0u8; 1];

        assert_eq!(
            r.read_exact(&mut buf).await.expect_err("must fail").kind(),
            std::io::ErrorKind::UnexpectedEof
        );

        assert_eq!(&[0], &buf, "buffer should stay empty");
    }

    /// Start a 9 bytes payload packet, but have the underlying reader return
    /// EOF in the middle of the payload (4 bytes into the payload).
    /// We should get an unexpected EOF error, after reading the first 4 bytes
    /// (successfully).
    #[tokio::test]
    async fn read_9b_eof_during_payload() {
        let payload = &hex!("FF0102030405060708");
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..8 + 4])
            .build();

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);
        let mut buf = [0; 9];

        r.read_exact(&mut buf[..4]).await.expect("must succeed");

        assert_eq!(
            r.read_exact(&mut buf[4..=4])
                .await
                .expect_err("must fail")
                .kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    /// Start a 9 bytes payload packet, but don't supply the necessary padding.
    /// This is expected to always fail before returning the final data.
    #[rstest]
    #[case::before_padding(8 + 9)]
    #[case::during_padding(8 + 9 + 2)]
    #[case::after_padding(8 + 9 + padding_len(9) as usize - 1)]
    #[tokio::test]
    async fn read_9b_eof_after_payload(#[case] offset: usize) {
        let payload = &hex!("FF0102030405060708");
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..offset])
            .build();

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);

        // read_exact of the payload *body* will succeed, but a subsequent read will
        // return UnexpectedEof error.
        assert_eq!(r.read_exact(&mut [0; 8]).await.unwrap(), 8);
        assert_eq!(
            r.read_exact(&mut [0]).await.unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }

    /// Start a 9 bytes payload packet, but return an error after a certain position.
    /// Ensure that error is propagated.
    #[rstest]
    #[case::during_size(4)]
    #[case::before_payload(8)]
    #[case::during_payload(8 + 4)]
    #[case::before_padding(8 + 4)]
    #[case::during_padding(8 + 9 + 2)]
    #[tokio::test]
    async fn propagate_error_from_reader(#[case] offset: usize) {
        let payload = &hex!("FF0102030405060708");
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..offset])
            .read_error(std::io::Error::new(std::io::ErrorKind::Other, "foo"))
            .build();

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);
        let mut buf = Vec::new();

        let err = r.read_to_end(&mut buf).await.expect_err("must fail");
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::Other,
            "error kind must match"
        );

        assert_eq!(
            err.into_inner().unwrap().to_string(),
            "foo",
            "error payload must contain foo"
        );
    }

    /// If there's an error right after the padding, we don't propagate it, as
    /// we're done reading. We just return EOF.
    #[tokio::test]
    async fn no_error_after_eof() {
        let payload = &hex!("FF0102030405060708");
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await)
            .read_error(std::io::Error::new(std::io::ErrorKind::Other, "foo"))
            .build();

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);
        let mut buf = Vec::new();

        r.read_to_end(&mut buf).await.expect("must succeed");
        assert_eq!(buf.as_slice(), payload);
    }

    /// Introduce various stalls in various places of the packet, to ensure we
    /// handle these cases properly, too.
    #[rstest]
    #[case::beginning(0)]
    #[case::before_payload(8)]
    #[case::during_payload(8 + 4)]
    #[case::before_padding(8 + 4)]
    #[case::during_padding(8 + 9 + 2)]
    #[tokio::test]
    async fn read_payload_correct_pending(#[case] offset: usize) {
        let payload = &hex!("FF0102030405060708");
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..offset])
            .wait(Duration::from_nanos(0))
            .read(&produce_packet_bytes(payload).await[offset..])
            .build();

        let mut r = BytesReader::new(&mut mock, ..=LARGE_PAYLOAD.len() as u64);
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.expect("must succeed");

        assert_eq!(payload, &buf[..]);
    }
}
