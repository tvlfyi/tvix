use pin_project_lite::pin_project;
use std::{
    ops::RangeBounds,
    task::{ready, Poll},
};
use tokio::io::AsyncRead;

use crate::wire::bytes::padding_len;

use super::bytes_writer::{BytesPacketPosition, LEN_SIZE};

pin_project! {
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
    /// It will only signal EOF (returning `Ok(())` without filling the buffer anymore)
    /// when all padding has been successfully consumed too.
    ///
    /// This also means, it's important for a user to always read to the end,
    /// and not just call read_exact - otherwise it might not skip over the
    /// padding, and return garbage when reading the next packet.
    ///
    /// In case of an error due to size constraints, or in case of not reading
    /// all the way to the end (and getting a EOF), the underlying reader is no
    /// longer usable and might return garbage.
    pub struct BytesReader<R, S>
    where
    R: AsyncRead,
    S: RangeBounds<u64>,

    {
        #[pin]
        inner: R,

        allowed_size: S,
        payload_size: [u8; 8],
        state: BytesPacketPosition,
    }
}

impl<R, S> BytesReader<R, S>
where
    R: AsyncRead + Unpin,
    S: RangeBounds<u64>,
{
    /// Constructs a new BytesReader, using the underlying passed reader.
    pub fn new(r: R, allowed_size: S) -> Self {
        Self {
            inner: r,
            allowed_size,
            payload_size: [0; 8],
            state: BytesPacketPosition::Size(0),
        }
    }
}
/// Returns an error if the passed usize is 0.
fn ensure_nonzero_bytes_read(bytes_read: usize) -> Result<usize, std::io::Error> {
    if bytes_read == 0 {
        Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "underlying reader returned EOF",
        ))
    } else {
        Ok(bytes_read)
    }
}

impl<R, S> AsyncRead for BytesReader<R, S>
where
    R: AsyncRead,
    S: RangeBounds<u64>,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut this = self.project();

        // Use a loop, so we can deal with (multiple) state transitions.
        loop {
            match *this.state {
                BytesPacketPosition::Size(LEN_SIZE) => {
                    // used in case an invalid size was signalled.
                    Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "signalled package size not in allowed range",
                    ))?
                }
                BytesPacketPosition::Size(pos) => {
                    // try to read more of the size field.
                    // We wrap a BufRead around this.payload_size here, and set_filled.
                    let mut read_buf = tokio::io::ReadBuf::new(this.payload_size);
                    read_buf.advance(pos);
                    ready!(this.inner.as_mut().poll_read(cx, &mut read_buf))?;

                    ensure_nonzero_bytes_read(read_buf.filled().len() - pos)?;

                    let total_size_read = read_buf.filled().len();
                    if total_size_read == LEN_SIZE {
                        // If the entire payload size was read, parse it
                        let payload_size = u64::from_le_bytes(*this.payload_size);

                        if !this.allowed_size.contains(&payload_size) {
                            // If it's not in the allowed
                            // range, transition to failure mode
                            // `BytesPacketPosition::Size(LEN_SIZE)`, where only
                            // an error is returned.
                            *this.state = BytesPacketPosition::Size(LEN_SIZE)
                        } else if payload_size == 0 {
                            // If the payload size is 0, move on to reading padding directly.
                            *this.state = BytesPacketPosition::Padding(0)
                        } else {
                            // Else, transition to reading the payload.
                            *this.state = BytesPacketPosition::Payload(0)
                        }
                    } else {
                        // If we still need to read more of payload size, update
                        // our position in the state.
                        *this.state = BytesPacketPosition::Size(total_size_read)
                    }
                }
                BytesPacketPosition::Payload(pos) => {
                    let signalled_size = u64::from_le_bytes(*this.payload_size);
                    // We don't enter this match arm at all if we're expecting empty payload
                    debug_assert!(signalled_size > 0, "signalled size must be larger than 0");

                    // Read from the underlying reader into buf
                    // We cap the ReadBuf to the size of the payload, as we
                    // don't want to leak padding to the caller.
                    let bytes_read = ensure_nonzero_bytes_read({
                        // Reducing these two u64 to usize on 32bits is fine - we
                        // only care about not reading too much, not too less.
                        let mut limited_buf = buf.take((signalled_size - pos) as usize);
                        ready!(this.inner.as_mut().poll_read(cx, &mut limited_buf))?;
                        limited_buf.filled().len()
                    })?;

                    // SAFETY: we just did populate this, but through limited_buf.
                    unsafe { buf.assume_init(bytes_read) }
                    buf.advance(bytes_read);

                    if pos + bytes_read as u64 == signalled_size {
                        // If we now read all payload, transition to padding
                        // state.
                        *this.state = BytesPacketPosition::Padding(0);
                    } else {
                        // if we didn't read everything yet, update our position
                        // in the state.
                        *this.state = BytesPacketPosition::Payload(pos + bytes_read as u64);
                    }

                    // We return from poll_read here.
                    // This is important, as any error (or even Pending) from
                    // the underlying reader on the next read (be it padding or
                    // payload) would require us to roll back buf, as generally
                    // a AsyncRead::poll_read may not advance the buffer in case
                    // of a nonsuccessful read.
                    // It can't be misinterpreted as EOF, as we definitely *did*
                    // write something into buf if we come to here (we pass
                    // `ensure_nonzero_bytes_read`).
                    return Ok(()).into();
                }
                BytesPacketPosition::Padding(pos) => {
                    // Consume whatever padding is left, ensuring it's all null
                    // bytes. Only return `Ready(Ok(()))` once we're past the
                    // padding (or in cases where polling the inner reader
                    // returns `Poll::Pending`).
                    let signalled_size = u64::from_le_bytes(*this.payload_size);
                    let total_padding_len = padding_len(signalled_size) as usize;

                    let padding_len_remaining = total_padding_len - pos;
                    if padding_len_remaining != 0 {
                        // create a buffer only accepting the number of remaining padding bytes.
                        let mut buf = [0; 8];
                        let mut padding_buf = tokio::io::ReadBuf::new(&mut buf);
                        let mut padding_buf = padding_buf.take(padding_len_remaining);

                        // read into padding_buf.
                        ready!(this.inner.as_mut().poll_read(cx, &mut padding_buf))?;
                        let bytes_read = ensure_nonzero_bytes_read(padding_buf.filled().len())?;

                        *this.state = BytesPacketPosition::Padding(pos + bytes_read);

                        // ensure the bytes are not null bytes
                        if !padding_buf.filled().iter().all(|e| *e == b'\0') {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "padding is not all zeroes",
                            ))
                            .into();
                        }

                        // if we still have padding to read, run the loop again.
                        continue;
                    }
                    // return EOF
                    return Ok(()).into();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::wire::bytes::write_bytes;
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
    #[case::size_9b( &hex!("000102030405060708"))] // 9 bytes payload (7 bytes padding)
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

    /// Start a 9 bytes payload packet, but return an error at various stages *after* the actual payload.
    /// read_exact with a 9 bytes buffer is expected to succeed, but any further
    /// read, as well as read_to_end are expected to fail.
    #[rstest]
    #[case::before_padding(8 + 9)]
    #[case::during_padding(8 + 9 + 2)]
    #[case::after_padding(8 + 9 + padding_len(9) as usize)]
    #[tokio::test]
    async fn read_9b_eof_after_payload(#[case] offset: usize) {
        let payload = &hex!("FF0102030405060708");
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..offset])
            .build();

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);
        let mut buf = [0; 9];

        // read_exact of the payload will succeed, but a subsequent read will
        // return UnexpectedEof error.
        r.read_exact(&mut buf).await.expect("should succeed");
        assert_eq!(
            r.read_exact(&mut buf[4..=4])
                .await
                .expect_err("must fail")
                .kind(),
            std::io::ErrorKind::UnexpectedEof
        );

        // read_to_end will fail.
        let mut mock = Builder::new()
            .read(&produce_packet_bytes(payload).await[..8 + payload.len()])
            .build();

        let mut r = BytesReader::new(&mut mock, ..MAX_LEN);
        let mut buf = Vec::new();
        assert_eq!(
            r.read_to_end(&mut buf).await.expect_err("must fail").kind(),
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
