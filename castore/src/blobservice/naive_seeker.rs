use super::BlobReader;
use futures::ready;
use pin_project_lite::pin_project;
use std::io;
use std::task::Poll;
use tokio::io::AsyncRead;
use tracing::{debug, instrument, trace, warn};

pin_project! {
    /// This implements [tokio::io::AsyncSeek] for and [tokio::io::AsyncRead] by
    /// simply skipping over some bytes, keeping track of the position.
    /// It fails whenever you try to seek backwards.
    ///
    /// ## Pinning concerns:
    ///
    /// [NaiveSeeker] is itself pinned by callers, and we do not need to concern
    /// ourselves regarding that.
    ///
    /// Though, its fields as per
    /// <https://doc.rust-lang.org/std/pin/#pinning-is-not-structural-for-field>
    /// can be pinned or unpinned.
    ///
    /// So we need to go over each field and choose our policy carefully.
    ///
    /// The obvious cases are the bookkeeping integers we keep in the structure,
    /// those are private and not shared to anyone, we never build a
    /// `Pin<&mut X>` out of them at any point, therefore, we can safely never
    /// mark them as pinned. Of course, it is expected that no developer here
    /// attempt to `pin!(self.pos)` to pin them because it makes no sense. If
    /// they have to become pinned, they should be marked `#[pin]` and we need
    /// to discuss it.
    ///
    /// So the bookkeeping integers are in the right state with respect to their
    /// pinning status. The projection should offer direct access.
    ///
    /// On the `r` field, i.e. a `BufReader<R>`, given that
    /// <https://docs.rs/tokio/latest/tokio/io/struct.BufReader.html#impl-Unpin-for-BufReader%3CR%3E>
    /// is available, even a `Pin<&mut BufReader<R>>` can be safely moved.
    ///
    /// The only care we should have regards the internal reader itself, i.e.
    /// the `R` instance, see that Tokio decided to `#[pin]` it too:
    /// <https://docs.rs/tokio/latest/src/tokio/io/util/buf_reader.rs.html#29>
    ///
    /// In general, there's no `Unpin` instance for `R: tokio::io::AsyncRead`
    /// (see <https://docs.rs/tokio/latest/tokio/io/trait.AsyncRead.html>).
    ///
    /// Therefore, we could keep it unpinned and pin it in every call site
    /// whenever we need to call `poll_*` which can be confusing to the non-
    /// expert developer and we have a fair share amount of situations where the
    /// [BufReader] instance is naked, i.e. in its `&mut BufReader<R>`
    /// form, this is annoying because it could lead to expose the naked `R`
    /// internal instance somehow and would produce a risk of making it move
    /// unexpectedly.
    ///
    /// We choose the path of the least resistance as we have no reason to have
    /// access to the raw `BufReader<R>` instance, we just `#[pin]` it too and
    /// enjoy its `poll_*` safe APIs and push the unpinning concerns to the
    /// internal implementations themselves, which studied the question longer
    /// than us.
    pub struct NaiveSeeker<R: tokio::io::AsyncRead> {
        #[pin]
        r: tokio::io::BufReader<R>,
        pos: u64,
        bytes_to_skip: u64,
    }
}

/// The buffer size used to discard data.
const DISCARD_BUF_SIZE: usize = 4096;

impl<R: tokio::io::AsyncRead> NaiveSeeker<R> {
    pub fn new(r: R) -> Self {
        NaiveSeeker {
            r: tokio::io::BufReader::new(r),
            pos: 0,
            bytes_to_skip: 0,
        }
    }
}

impl<R: tokio::io::AsyncRead> tokio::io::AsyncRead for NaiveSeeker<R> {
    #[instrument(level = "trace", skip_all)]
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        // The amount of data read can be determined by the increase
        // in the length of the slice returned by `ReadBuf::filled`.
        let filled_before = buf.filled().len();

        let this = self.project();
        ready!(this.r.poll_read(cx, buf))?;

        let bytes_read = buf.filled().len() - filled_before;
        *this.pos += bytes_read as u64;

        trace!(bytes_read = bytes_read, new_pos = this.pos, "poll_read");

        Ok(()).into()
    }
}

impl<R: tokio::io::AsyncRead> tokio::io::AsyncBufRead for NaiveSeeker<R> {
    fn poll_fill_buf(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<io::Result<&[u8]>> {
        self.project().r.poll_fill_buf(cx)
    }

    #[instrument(level = "trace", skip(self))]
    fn consume(self: std::pin::Pin<&mut Self>, amt: usize) {
        let this = self.project();
        this.r.consume(amt);
        *this.pos += amt as u64;

        trace!(new_pos = this.pos, "consume");
    }
}

impl<R: tokio::io::AsyncRead> tokio::io::AsyncSeek for NaiveSeeker<R> {
    #[instrument(level="trace", skip(self), fields(inner_pos=%self.pos), err(Debug))]
    fn start_seek(
        self: std::pin::Pin<&mut Self>,
        position: std::io::SeekFrom,
    ) -> std::io::Result<()> {
        let absolute_offset: u64 = match position {
            io::SeekFrom::Start(start_offset) => {
                if start_offset < self.pos {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!("can't seek backwards ({} -> {})", self.pos, start_offset),
                    ));
                } else {
                    start_offset
                }
            }
            // we don't know the total size, can't support this.
            io::SeekFrom::End(_end_offset) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "can't seek from end",
                ));
            }
            io::SeekFrom::Current(relative_offset) => {
                if relative_offset < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "can't seek backwards relative to current position",
                    ));
                } else {
                    self.pos + relative_offset as u64
                }
            }
        };

        // we already know absolute_offset is >= self.pos
        debug_assert!(
            absolute_offset >= self.pos,
            "absolute_offset {} must be >= self.pos {}",
            absolute_offset,
            self.pos
        );

        // calculate bytes to skip
        let this = self.project();
        *this.bytes_to_skip = absolute_offset - *this.pos;

        debug!(bytes_to_skip = *this.bytes_to_skip, "seek");

        Ok(())
    }

    #[instrument(skip_all)]
    fn poll_complete(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<u64>> {
        if self.bytes_to_skip == 0 {
            // return the new position (from the start of the stream)
            return Poll::Ready(Ok(self.pos));
        }

        // discard some bytes, until pos is where we want it to be.
        // We create a buffer that we'll discard later on.
        let mut discard_buf = [0; DISCARD_BUF_SIZE];

        // Loop until we've reached the desired seek position. This is done by issuing repeated
        // `poll_read` calls.
        // If the data is not available yet, we will yield back to the executor
        // and wait to be polled again.
        loop {
            if self.bytes_to_skip == 0 {
                return Poll::Ready(Ok(self.pos));
            }

            // calculate the length we want to skip at most, which is either a max
            // buffer size, or the number of remaining bytes to read, whatever is
            // smaller.
            let bytes_to_skip_now = std::cmp::min(self.bytes_to_skip as usize, discard_buf.len());
            let mut discard_buf = tokio::io::ReadBuf::new(&mut discard_buf[..bytes_to_skip_now]);

            ready!(self.as_mut().poll_read(cx, &mut discard_buf))?;
            let bytes_skipped = discard_buf.filled().len();

            if bytes_skipped == 0 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "got EOF while trying to skip bytes",
                )));
            }
            // decrement bytes to skip. The poll_read call already updated self.pos.
            *self.as_mut().project().bytes_to_skip -= bytes_skipped as u64;
        }
    }
}

impl<R: tokio::io::AsyncRead + Send + Unpin + 'static> BlobReader for NaiveSeeker<R> {}

#[cfg(test)]
mod tests {
    use super::{NaiveSeeker, DISCARD_BUF_SIZE};
    use std::io::{Cursor, SeekFrom};
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    /// This seek requires multiple `poll_read` as we use a multiples of
    /// DISCARD_BUF_SIZE when doing the seek.
    /// This ensures we don't hang indefinitely.
    #[tokio::test]
    async fn seek() {
        let buf = vec![0u8; DISCARD_BUF_SIZE * 4];
        let reader = Cursor::new(&buf);
        let mut seeker = NaiveSeeker::new(reader);
        seeker.seek(SeekFrom::Start(4000)).await.unwrap();
    }

    #[tokio::test]
    async fn seek_read() {
        let mut buf = vec![0u8; DISCARD_BUF_SIZE * 2];
        buf.extend_from_slice(&[1u8; DISCARD_BUF_SIZE * 2]);
        buf.extend_from_slice(&[2u8; DISCARD_BUF_SIZE * 2]);

        let reader = Cursor::new(&buf);
        let mut seeker = NaiveSeeker::new(reader);

        let mut read_buf = vec![0u8; DISCARD_BUF_SIZE];
        seeker.read_exact(&mut read_buf).await.expect("must read");
        assert_eq!(read_buf.as_slice(), &[0u8; DISCARD_BUF_SIZE]);

        seeker
            .seek(SeekFrom::Current(DISCARD_BUF_SIZE as i64))
            .await
            .expect("must seek");
        seeker.read_exact(&mut read_buf).await.expect("must read");
        assert_eq!(read_buf.as_slice(), &[1u8; DISCARD_BUF_SIZE]);

        seeker
            .seek(SeekFrom::Start(2 * 2 * DISCARD_BUF_SIZE as u64))
            .await
            .expect("must seek");
        seeker.read_exact(&mut read_buf).await.expect("must read");
        assert_eq!(read_buf.as_slice(), &[2u8; DISCARD_BUF_SIZE]);
    }
}
