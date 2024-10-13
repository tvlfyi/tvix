use pin_project_lite::pin_project;
use std::task::{ready, Poll};

use tokio::io::AsyncWrite;

use super::{padding_len, EMPTY_BYTES, LEN_SIZE};

pin_project! {
    /// Writes a "bytes wire packet" to the underlying writer.
    /// The format is the same as in [crate::wire::bytes::write_bytes],
    /// however this structure provides a [AsyncWrite] interface,
    /// allowing to not having to pass around the entire payload in memory.
    ///
    /// It internally takes care of writing (non-payload) framing (size and
    /// padding).
    ///
    /// During construction, the expected payload size needs to be provided.
    ///
    /// After writing the payload to it, the user MUST call flush (or shutdown),
    /// which will validate the written payload size to match, and write the
    /// necessary padding.
    ///
    /// In case flush is not called at the end, invalid data might be sent
    /// silently.
    ///
    /// The underlying writer returning `Ok(0)` is considered an EOF situation,
    /// which is stronger than the "typically means the underlying object is no
    /// longer able to accept bytes" interpretation from the docs. If such a
    /// situation occurs, an error is returned.
    ///
    /// The struct holds three fields, the underlying writer, the (expected)
    /// payload length, and an enum, tracking the state.
    pub struct BytesWriter<W>
    where
        W: AsyncWrite,
    {
        #[pin]
        inner: W,
        payload_len: u64,
        state: BytesPacketPosition,
    }
}

/// Models the position inside a "bytes wire packet" that the writer is in.
/// It can be in three different stages, inside size, payload or padding fields.
/// The number tracks the number of bytes written inside the specific field.
/// There shall be no ambiguous states, at the end of a stage we immediately
/// move to the beginning of the next one:
/// - Size(LEN_SIZE) must be expressed as Payload(0)
/// - Payload(self.payload_len) must be expressed as Padding(0)
///
/// Padding(padding_len) means we're at the end of the bytes wire packet.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BytesPacketPosition {
    Size(usize),
    Payload(u64),
    Padding(usize),
}

impl<W> BytesWriter<W>
where
    W: AsyncWrite,
{
    /// Constructs a new BytesWriter, using the underlying passed writer.
    pub fn new(w: W, payload_len: u64) -> Self {
        Self {
            inner: w,
            payload_len,
            state: BytesPacketPosition::Size(0),
        }
    }
}

/// Returns an error if the passed usize is 0.
#[inline]
fn ensure_nonzero_bytes_written(bytes_written: usize) -> Result<usize, std::io::Error> {
    if bytes_written == 0 {
        Err(std::io::Error::new(
            std::io::ErrorKind::WriteZero,
            "underlying writer accepted 0 bytes",
        ))
    } else {
        Ok(bytes_written)
    }
}

impl<W> AsyncWrite for BytesWriter<W>
where
    W: AsyncWrite,
{
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        // Use a loop, so we can deal with (multiple) state transitions.
        let mut this = self.project();

        loop {
            match *this.state {
                BytesPacketPosition::Size(LEN_SIZE) => unreachable!(),
                BytesPacketPosition::Size(pos) => {
                    let size_field = &this.payload_len.to_le_bytes();

                    let bytes_written = ensure_nonzero_bytes_written(ready!(this
                        .inner
                        .as_mut()
                        .poll_write(cx, &size_field[pos..]))?)?;

                    let new_pos = pos + bytes_written;
                    if new_pos == LEN_SIZE {
                        *this.state = BytesPacketPosition::Payload(0);
                    } else {
                        *this.state = BytesPacketPosition::Size(new_pos);
                    }
                }
                BytesPacketPosition::Payload(pos) => {
                    // Ensure we still have space for more payload
                    if pos + (buf.len() as u64) > *this.payload_len {
                        return Poll::Ready(Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "tried to write excess bytes",
                        )));
                    }
                    let bytes_written = ready!(this.inner.as_mut().poll_write(cx, buf))?;
                    ensure_nonzero_bytes_written(bytes_written)?;
                    let new_pos = pos + (bytes_written as u64);
                    if new_pos == *this.payload_len {
                        *this.state = BytesPacketPosition::Padding(0)
                    } else {
                        *this.state = BytesPacketPosition::Payload(new_pos)
                    }

                    return Poll::Ready(Ok(bytes_written));
                }
                // If we're already in padding state, there should be no more payload left to write!
                BytesPacketPosition::Padding(_pos) => {
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "tried to write excess bytes",
                    )))
                }
            }
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let mut this = self.project();

        loop {
            match *this.state {
                BytesPacketPosition::Size(LEN_SIZE) => unreachable!(),
                BytesPacketPosition::Size(pos) => {
                    // More bytes to write in the size field
                    let size_field = &this.payload_len.to_le_bytes()[..];
                    let bytes_written = ensure_nonzero_bytes_written(ready!(this
                        .inner
                        .as_mut()
                        .poll_write(cx, &size_field[pos..]))?)?;
                    let new_pos = pos + bytes_written;
                    if new_pos == LEN_SIZE {
                        // Size field written, now ready to receive payload
                        *this.state = BytesPacketPosition::Payload(0);
                    } else {
                        *this.state = BytesPacketPosition::Size(new_pos);
                    }
                }
                BytesPacketPosition::Payload(_pos) => {
                    // If we're at position 0 and want to write 0 bytes of payload
                    // in total, we can transition to padding.
                    // Otherwise, break, as we're expecting more payload to
                    // be written.
                    if *this.payload_len == 0 {
                        *this.state = BytesPacketPosition::Padding(0);
                    } else {
                        break;
                    }
                }
                BytesPacketPosition::Padding(pos) => {
                    // Write remaining padding, if there is padding to write.
                    let total_padding_len = padding_len(*this.payload_len) as usize;

                    if pos != total_padding_len {
                        let bytes_written = ensure_nonzero_bytes_written(ready!(this
                            .inner
                            .as_mut()
                            .poll_write(cx, &EMPTY_BYTES[pos..total_padding_len]))?)?;
                        *this.state = BytesPacketPosition::Padding(pos + bytes_written);
                    } else {
                        // everything written, break
                        break;
                    }
                }
            }
        }
        // Flush the underlying writer.
        this.inner.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        // Call flush.
        ready!(self.as_mut().poll_flush(cx))?;

        let this = self.project();

        // After a flush, being inside the padding state, and at the end of the padding
        // is the only way to prevent a dirty shutdown.
        if let BytesPacketPosition::Padding(pos) = *this.state {
            let padding_len = padding_len(*this.payload_len) as usize;
            if padding_len == pos {
                // Shutdown the underlying writer
                return this.inner.poll_shutdown(cx);
            }
        }

        // Shutdown the underlying writer, bubbling up any errors.
        ready!(this.inner.poll_shutdown(cx))?;

        // return an error about unclean shutdown
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "unclean shutdown",
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;
    use std::time::Duration;

    use crate::wire::bytes::write_bytes;
    use hex_literal::hex;
    use tokio::io::AsyncWriteExt;
    use tokio_test::{assert_err, assert_ok, io::Builder};

    use super::*;

    pub static LARGE_PAYLOAD: LazyLock<Vec<u8>> =
        LazyLock::new(|| (0..255).collect::<Vec<u8>>().repeat(4 * 1024));

    /// Helper function, calling the (simpler) write_bytes with the payload.
    /// We use this to create data we want to see on the wire.
    async fn produce_exp_bytes(payload: &[u8]) -> Vec<u8> {
        let mut exp = vec![];
        write_bytes(&mut exp, payload).await.unwrap();
        exp
    }

    /// Write an empty bytes packet.
    #[tokio::test]
    async fn write_empty() {
        let payload = &[];
        let mut mock = Builder::new()
            .write(&produce_exp_bytes(payload).await)
            .build();

        let mut w = BytesWriter::new(&mut mock, 0);
        assert_ok!(w.write_all(&[]).await, "write all data");
        assert_ok!(w.flush().await, "flush");
    }

    /// Write an empty bytes packet, not calling write.
    #[tokio::test]
    async fn write_empty_only_flush() {
        let payload = &[];
        let mut mock = Builder::new()
            .write(&produce_exp_bytes(payload).await)
            .build();

        let mut w = BytesWriter::new(&mut mock, 0);
        assert_ok!(w.flush().await, "flush");
    }

    /// Write an empty bytes packet, not calling write or flush, only shutdown.
    #[tokio::test]
    async fn write_empty_only_shutdown() {
        let payload = &[];
        let mut mock = Builder::new()
            .write(&produce_exp_bytes(payload).await)
            .build();

        let mut w = BytesWriter::new(&mut mock, 0);
        assert_ok!(w.shutdown().await, "shutdown");
    }

    /// Write a 1 bytes packet
    #[tokio::test]
    async fn write_1b() {
        let payload = &[0xff];

        let mut mock = Builder::new()
            .write(&produce_exp_bytes(payload).await)
            .build();

        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);
        assert_ok!(w.write_all(payload).await);
        assert_ok!(w.flush().await, "flush");
    }

    /// Write a 8 bytes payload (no padding)
    #[tokio::test]
    async fn write_8b() {
        let payload = &hex!("0001020304050607");

        let mut mock = Builder::new()
            .write(&produce_exp_bytes(payload).await)
            .build();

        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);
        assert_ok!(w.write_all(payload).await);
        assert_ok!(w.flush().await, "flush");
    }

    /// Write a 9 bytes payload (7 bytes padding)
    #[tokio::test]
    async fn write_9b() {
        let payload = &hex!("000102030405060708");

        let mut mock = Builder::new()
            .write(&produce_exp_bytes(payload).await)
            .build();

        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);
        assert_ok!(w.write_all(payload).await);
        assert_ok!(w.flush().await, "flush");
    }

    /// Write a 9 bytes packet very granularly, with a lot of flushing in between,
    /// and a shutdown at the end.
    #[tokio::test]
    async fn write_9b_flush() {
        let payload = &hex!("000102030405060708");
        let exp_bytes = produce_exp_bytes(payload).await;

        let mut mock = Builder::new().write(&exp_bytes).build();

        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);
        assert_ok!(w.flush().await);

        assert_ok!(w.write_all(&payload[..4]).await);
        assert_ok!(w.flush().await);

        // empty write, cause why not
        assert_ok!(w.write_all(&[]).await);
        assert_ok!(w.flush().await);

        assert_ok!(w.write_all(&payload[4..]).await);
        assert_ok!(w.flush().await);
        assert_ok!(w.shutdown().await);
    }

    /// Write a 9 bytes packet, but cause the sink to only accept half of the
    /// padding, ensuring we correctly write (only) the rest of the padding later.
    /// We write another 2 bytes of "bait", where a faulty implementation (pre
    /// cl/11384) would put too many null bytes.
    #[tokio::test]
    async fn write_9b_write_padding_2steps() {
        let payload = &hex!("000102030405060708");
        let exp_bytes = produce_exp_bytes(payload).await;

        let mut mock = Builder::new()
            .write(&exp_bytes[0..8]) // size
            .write(&exp_bytes[8..17]) // payload
            .write(&exp_bytes[17..19]) // padding (2 of 7 bytes)
            // insert a wait to prevent Mock from merging the two writes into one
            .wait(Duration::from_nanos(1))
            .write(&hex!("0000000000ffff")) // padding (5 of 7 bytes, plus 2 bytes of "bait")
            .build();

        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);
        assert_ok!(w.write_all(&payload[..]).await);
        assert_ok!(w.flush().await);
        // Write bait
        assert_ok!(mock.write_all(&hex!("ffff")).await);
    }

    /// Write a larger bytes packet
    #[tokio::test]
    async fn write_1m() {
        let payload = LARGE_PAYLOAD.as_slice();
        let exp_bytes = produce_exp_bytes(payload).await;

        let mut mock = Builder::new().write(&exp_bytes).build();
        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);

        assert_ok!(w.write_all(payload).await);
        assert_ok!(w.flush().await, "flush");
    }

    /// Not calling flush at the end, but shutdown is also ok if we wrote all
    /// bytes we promised to write (as shutdown implies flush)
    #[tokio::test]
    async fn write_shutdown_without_flush_end() {
        let payload = &[0xf0, 0xff];
        let exp_bytes = produce_exp_bytes(payload).await;

        let mut mock = Builder::new().write(&exp_bytes).build();
        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);

        // call flush to write the size field
        assert_ok!(w.flush().await);

        // write payload
        assert_ok!(w.write_all(payload).await);

        // call shutdown
        assert_ok!(w.shutdown().await);
    }

    /// Writing more bytes than previously signalled should fail.
    #[tokio::test]
    async fn write_more_than_signalled_fail() {
        let mut buf = Vec::new();
        let mut w = BytesWriter::new(&mut buf, 2);

        assert_err!(w.write_all(&hex!("000102")).await);
    }
    /// Writing more bytes than previously signalled, but in two parts
    #[tokio::test]
    async fn write_more_than_signalled_split_fail() {
        let mut buf = Vec::new();
        let mut w = BytesWriter::new(&mut buf, 2);

        // write two bytes
        assert_ok!(w.write_all(&hex!("0001")).await);

        // write the excess byte.
        assert_err!(w.write_all(&hex!("02")).await);
    }

    /// Writing more bytes than previously signalled, but flushing after the
    /// signalled amount should fail.
    #[tokio::test]
    async fn write_more_than_signalled_flush_fail() {
        let mut buf = Vec::new();
        let mut w = BytesWriter::new(&mut buf, 2);

        // write two bytes, then flush
        assert_ok!(w.write_all(&hex!("0001")).await);
        assert_ok!(w.flush().await);

        // write the excess byte.
        assert_err!(w.write_all(&hex!("02")).await);
    }

    /// Calling shutdown while not having written all bytes that were promised
    /// returns an error.
    /// Note there's still cases of silent corruption if the user doesn't call
    /// shutdown explicitly (only drops).
    #[tokio::test]
    async fn premature_shutdown() {
        let payload = &[0xf0, 0xff];
        let mut buf = Vec::new();
        let mut w = BytesWriter::new(&mut buf, payload.len() as u64);

        // call flush to write the size field
        assert_ok!(w.flush().await);

        // write half of the payload (!)
        assert_ok!(w.write_all(&payload[0..1]).await);

        // call shutdown, ensure it fails
        assert_err!(w.shutdown().await);
    }

    /// Write to a Writer that fails to write during the size packet (after 4 bytes).
    /// Ensure this error gets propagated on the first call to write.
    #[tokio::test]
    async fn inner_writer_fail_during_size_firstwrite() {
        let payload = &[0xf0];

        let mut mock = Builder::new()
            .write(&1u32.to_le_bytes())
            .write_error(std::io::Error::new(std::io::ErrorKind::Other, "🍿"))
            .build();
        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);

        assert_err!(w.write_all(payload).await);
    }

    /// Write to a Writer that fails to write during the size packet (after 4 bytes).
    /// Ensure this error gets propagated during an initial flush
    #[tokio::test]
    async fn inner_writer_fail_during_size_initial_flush() {
        let payload = &[0xf0];

        let mut mock = Builder::new()
            .write(&1u32.to_le_bytes())
            .write_error(std::io::Error::new(std::io::ErrorKind::Other, "🍿"))
            .build();
        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);

        assert_err!(w.flush().await);
    }

    /// Write to a writer that fails to write during the payload (after 9 bytes).
    /// Ensure this error gets propagated when we're writing this byte.
    #[tokio::test]
    async fn inner_writer_fail_during_write() {
        let payload = &hex!("f0ff");

        let mut mock = Builder::new()
            .write(&2u64.to_le_bytes())
            .write(&hex!("f0"))
            .write_error(std::io::Error::new(std::io::ErrorKind::Other, "🍿"))
            .build();
        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);

        assert_ok!(w.write(&hex!("f0")).await);
        assert_err!(w.write(&hex!("ff")).await);
    }

    /// Write to a writer that fails to write during the padding (after 10 bytes).
    /// Ensure this error gets propagated during a flush.
    #[tokio::test]
    async fn inner_writer_fail_during_padding_flush() {
        let payload = &hex!("f0");

        let mut mock = Builder::new()
            .write(&1u64.to_le_bytes())
            .write(&hex!("f0"))
            .write(&hex!("00"))
            .write_error(std::io::Error::new(std::io::ErrorKind::Other, "🍿"))
            .build();
        let mut w = BytesWriter::new(&mut mock, payload.len() as u64);

        assert_ok!(w.write(&hex!("f0")).await);
        assert_err!(w.flush().await);
    }
}
