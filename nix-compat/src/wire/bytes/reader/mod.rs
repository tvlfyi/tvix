use std::{
    future::Future,
    io,
    num::NonZeroU64,
    ops::RangeBounds,
    pin::Pin,
    task::{self, ready, Poll},
};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt, ReadBuf};

use trailer::{read_trailer, ReadTrailer, Trailer};

#[doc(hidden)]
pub use self::trailer::Pad;
pub(crate) use self::trailer::Tag;
mod trailer;

/// Reads a "bytes wire packet" from the underlying reader.
/// The format is the same as in [crate::wire::bytes::read_bytes],
/// however this structure provides a [AsyncRead] interface,
/// allowing to not having to pass around the entire payload in memory.
///
/// It is constructed by reading a size with [BytesReader::new],
/// and yields payload data until the end of the packet is reached.
///
/// It will not return the final bytes before all padding has been successfully
/// consumed as well, but the full length of the reader must be consumed.
///
/// If the data is not read all the way to the end, or an error is encountered,
/// the underlying reader is no longer usable and might return garbage.
#[derive(Debug)]
#[allow(private_bounds)]
pub struct BytesReader<R, T: Tag = Pad> {
    state: State<R, T>,
}

/// Split the `user_len` into `body_len` and `tail_len`, which are respectively
/// the non-terminal 8-byte blocks, and the ≤8 bytes of user data contained in
/// the trailer block.
#[inline(always)]
fn split_user_len(user_len: NonZeroU64) -> (u64, u8) {
    let n = user_len.get() - 1;
    let body_len = n & !7;
    let tail_len = (n & 7) as u8 + 1;
    (body_len, tail_len)
}

#[derive(Debug)]
enum State<R, T: Tag> {
    /// Full 8-byte blocks are being read and released to the caller.
    /// NOTE: The final 8-byte block is *always* part of the trailer.
    Body {
        reader: Option<R>,
        consumed: u64,
        /// The total length of all user data contained in both the body and trailer.
        user_len: NonZeroU64,
    },
    /// The trailer is in the process of being read.
    ReadTrailer(ReadTrailer<R, T>),
    /// The trailer has been fully read and validated,
    /// and data can now be released to the caller.
    ReleaseTrailer { consumed: u8, data: Trailer },
}

impl<R> BytesReader<R>
where
    R: AsyncRead + Unpin,
{
    /// Constructs a new BytesReader, using the underlying passed reader.
    pub async fn new<S: RangeBounds<u64>>(reader: R, allowed_size: S) -> io::Result<Self> {
        BytesReader::new_internal(reader, allowed_size).await
    }
}

#[allow(private_bounds)]
impl<R, T: Tag> BytesReader<R, T>
where
    R: AsyncRead + Unpin,
{
    /// Constructs a new BytesReader, using the underlying passed reader.
    pub(crate) async fn new_internal<S: RangeBounds<u64>>(
        mut reader: R,
        allowed_size: S,
    ) -> io::Result<Self> {
        let size = reader.read_u64_le().await?;

        if !allowed_size.contains(&size) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid size"));
        }

        Ok(Self {
            state: match NonZeroU64::new(size) {
                Some(size) => State::Body {
                    reader: Some(reader),
                    consumed: 0,
                    user_len: size,
                },
                None => State::ReleaseTrailer {
                    consumed: 0,
                    data: read_trailer::<R, T>(reader, 0).await?,
                },
            },
        })
    }

    /// Returns whether there is any remaining data to be read.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Remaining data length, ie not including data already read.
    ///
    /// If the size has not been read yet, this is [None].
    pub fn len(&self) -> u64 {
        match self.state {
            State::Body {
                consumed, user_len, ..
            } => user_len.get() - consumed,
            State::ReadTrailer(ref fut) => fut.len() as u64,
            State::ReleaseTrailer { consumed, ref data } => data.len() as u64 - consumed as u64,
        }
    }
}

#[allow(private_bounds)]
impl<R: AsyncRead + Unpin, T: Tag> AsyncRead for BytesReader<R, T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let this = &mut self.state;

        loop {
            match this {
                State::Body {
                    reader,
                    consumed,
                    user_len,
                } => {
                    let (body_len, tail_len) = split_user_len(*user_len);
                    let remaining = body_len - *consumed;

                    let reader = if remaining == 0 {
                        let reader = reader.take().unwrap();
                        *this = State::ReadTrailer(read_trailer(reader, tail_len));
                        continue;
                    } else {
                        Pin::new(reader.as_mut().unwrap())
                    };

                    let mut bytes_read = 0;
                    ready!(with_limited(buf, remaining, |buf| {
                        let ret = reader.poll_read(cx, buf);
                        bytes_read = buf.filled().len();
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

#[allow(private_bounds)]
impl<R: AsyncBufRead + Unpin, T: Tag> AsyncBufRead for BytesReader<R, T> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut task::Context) -> Poll<io::Result<&[u8]>> {
        let this = &mut self.get_mut().state;

        loop {
            match this {
                // This state comes *after* the following case,
                // but we can't keep it in logical order because
                // that would lengthen the borrow lifetime.
                State::Body {
                    reader,
                    consumed,
                    user_len,
                } if {
                    let (body_len, _) = split_user_len(*user_len);
                    let remaining = body_len - *consumed;

                    remaining == 0
                } =>
                {
                    let reader = reader.take().unwrap();
                    let (_, tail_len) = split_user_len(*user_len);

                    *this = State::ReadTrailer(read_trailer(reader, tail_len));
                }
                State::Body {
                    reader,
                    consumed,
                    user_len,
                } => {
                    let (body_len, _) = split_user_len(*user_len);
                    let remaining = body_len - *consumed;

                    let reader = Pin::new(reader.as_mut().unwrap());

                    match ready!(reader.poll_fill_buf(cx))? {
                        &[] => {
                            return Err(io::ErrorKind::UnexpectedEof.into()).into();
                        }
                        mut buf => {
                            if buf.len() as u64 > remaining {
                                buf = &buf[..remaining as usize];
                            }

                            return Ok(buf).into();
                        }
                    }
                }
                State::ReadTrailer(fut) => {
                    *this = State::ReleaseTrailer {
                        consumed: 0,
                        data: ready!(Pin::new(fut).poll(cx))?,
                    };
                }
                State::ReleaseTrailer { consumed, data } => {
                    return Ok(&data[*consumed as usize..]).into();
                }
            }
        }
    }

    fn consume(mut self: Pin<&mut Self>, amt: usize) {
        match &mut self.state {
            State::Body {
                reader,
                consumed,
                user_len,
            } => {
                let reader = Pin::new(reader.as_mut().unwrap());
                let (body_len, _) = split_user_len(*user_len);

                *consumed = consumed
                    .checked_add(amt as u64)
                    .filter(|&consumed| consumed <= body_len)
                    .expect("consumed out of bounds");

                reader.consume(amt);
            }
            State::ReadTrailer(_) => unreachable!(),
            State::ReleaseTrailer { consumed, data } => {
                *consumed = amt
                    .checked_add(*consumed as usize)
                    .filter(|&consumed| consumed <= data.len())
                    .expect("consumed out of bounds") as u8;
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
    use tokio::io::{AsyncReadExt, BufReader};
    use tokio_test::io::Builder;

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

        let mut r = BytesReader::new(&mut mock, ..=LARGE_PAYLOAD.len() as u64)
            .await
            .unwrap();
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.expect("must succeed");

        assert_eq!(payload, &buf[..]);
    }

    /// Read bytes packets of various length, and ensure copy_buf reads the
    /// expected payload.
    #[rstest]
    #[case::empty(&[])] // empty bytes packet
    #[case::size_1b(&[0xff])] // 1 bytes payload
    #[case::size_8b(&hex!("0001020304050607"))] // 8 bytes payload (no padding)
    #[case::size_9b(&hex!("000102030405060708"))] // 9 bytes payload (7 bytes padding)
    #[case::size_1m(LARGE_PAYLOAD.as_slice())] // larger bytes packet
    #[tokio::test]
    async fn read_payload_correct_readbuf(#[case] payload: &[u8]) {
        let mut mock = BufReader::new(
            Builder::new()
                .read(&produce_packet_bytes(payload).await)
                .build(),
        );

        let mut r = BytesReader::new(&mut mock, ..=LARGE_PAYLOAD.len() as u64)
            .await
            .unwrap();

        let mut buf = Vec::new();
        tokio::io::copy_buf(&mut r, &mut buf)
            .await
            .expect("copy_buf must succeed");

        assert_eq!(payload, &buf[..]);
    }

    /// Fail if the bytes packet is larger than allowed
    #[tokio::test]
    async fn read_bigger_than_allowed_fail() {
        let payload = LARGE_PAYLOAD.as_slice();
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[0..8]) // We stop reading after the size packet
            .build();

        assert_eq!(
            BytesReader::new(&mut mock, ..2048)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    /// Fail if the bytes packet is smaller than allowed
    #[tokio::test]
    async fn read_smaller_than_allowed_fail() {
        let payload = &[0x00, 0x01, 0x02];
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[0..8]) // We stop reading after the size packet
            .build();

        assert_eq!(
            BytesReader::new(&mut mock, 1024..2048)
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    /// Read the trailer immediately if there is no payload.
    #[cfg(feature = "async")]
    #[tokio::test]
    async fn read_trailer_immediately() {
        use crate::nar::wire::PadPar;

        let mut mock = Builder::new()
            .read(&[0; 8])
            .read(&PadPar::PATTERN[8..])
            .build();

        BytesReader::<_, PadPar>::new_internal(&mut mock, ..)
            .await
            .unwrap();

        // The mock reader will panic if dropped without reading all data.
    }

    /// Read the trailer even if we only read the exact payload size.
    #[cfg(feature = "async")]
    #[tokio::test]
    async fn read_exact_trailer() {
        use crate::nar::wire::PadPar;

        let mut mock = Builder::new()
            .read(&16u64.to_le_bytes())
            .read(&[0x55; 16])
            .read(&PadPar::PATTERN[8..])
            .build();

        let mut reader = BytesReader::<_, PadPar>::new_internal(&mut mock, ..)
            .await
            .unwrap();

        let mut buf = [0; 16];
        reader.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, [0x55; 16]);

        // The mock reader will panic if dropped without reading all data.
    }

    /// Fail if the padding is not all zeroes
    #[tokio::test]
    async fn read_fail_if_nonzero_padding() {
        let payload = &[0x00, 0x01, 0x02];
        let mut packet_bytes = produce_packet_bytes(payload).await;
        // Flip some bits in the padding
        packet_bytes[12] = 0xff;
        let mut mock = Builder::new().read(&packet_bytes).build(); // We stop reading after the faulty bit

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN).await.unwrap();
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

        assert_eq!(
            BytesReader::new(&mut mock, ..MAX_LEN)
                .await
                .expect_err("must fail")
                .kind(),
            io::ErrorKind::UnexpectedEof
        );
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

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN).await.unwrap();
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

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN).await.unwrap();

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

        // Either length reading or data reading can fail, depending on which test case we're in.
        let err: io::Error = async {
            let mut r = BytesReader::new(&mut mock, ..MAX_LEN).await?;
            let mut buf = Vec::new();

            r.read_to_end(&mut buf).await?;

            Ok(())
        }
        .await
        .expect_err("must fail");

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

    /// Start a 9 bytes payload packet, but return an error after a certain position.
    /// Ensure that error is propagated (AsyncReadBuf case)
    #[rstest]
    #[case::during_size(4)]
    #[case::before_payload(8)]
    #[case::during_payload(8 + 4)]
    #[case::before_padding(8 + 4)]
    #[case::during_padding(8 + 9 + 2)]
    #[tokio::test]
    async fn propagate_error_from_reader_buffered(#[case] offset: usize) {
        let payload = &hex!("FF0102030405060708");
        let mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..offset])
            .read_error(std::io::Error::new(std::io::ErrorKind::Other, "foo"))
            .build();
        let mut mock = BufReader::new(mock);

        // Either length reading or data reading can fail, depending on which test case we're in.
        let err: io::Error = async {
            let mut r = BytesReader::new(&mut mock, ..MAX_LEN).await?;
            let mut buf = Vec::new();

            tokio::io::copy_buf(&mut r, &mut buf).await?;

            Ok(())
        }
        .await
        .expect_err("must fail");

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

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN).await.unwrap();
        let mut buf = Vec::new();

        r.read_to_end(&mut buf).await.expect("must succeed");
        assert_eq!(buf.as_slice(), payload);
    }

    /// If there's an error right after the padding, we don't propagate it, as
    /// we're done reading. We just return EOF.
    #[tokio::test]
    async fn no_error_after_eof_buffered() {
        let payload = &hex!("FF0102030405060708");
        let mock = Builder::new()
            .read(&produce_packet_bytes(payload).await)
            .read_error(std::io::Error::new(std::io::ErrorKind::Other, "foo"))
            .build();
        let mut mock = BufReader::new(mock);

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN).await.unwrap();
        let mut buf = Vec::new();

        tokio::io::copy_buf(&mut r, &mut buf)
            .await
            .expect("must succeed");
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

        let mut r = BytesReader::new(&mut mock, ..=LARGE_PAYLOAD.len() as u64)
            .await
            .unwrap();
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).await.expect("must succeed");

        assert_eq!(payload, &buf[..]);
    }
}
