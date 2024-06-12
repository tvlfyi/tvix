use std::{
    io, mem,
    pin::Pin,
    task::{Context, Poll},
};

use async_compression::tokio::bufread::{BzDecoder, GzipDecoder, XzDecoder};
use futures::ready;
use pin_project::pin_project;
use tokio::io::{AsyncBufRead, AsyncRead, BufReader, ReadBuf};

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
const BZIP2_MAGIC: [u8; 3] = *b"BZh";
const XZ_MAGIC: [u8; 6] = [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00];
const BYTES_NEEDED: usize = 6;

#[derive(Debug, Clone, Copy)]
enum Algorithm {
    Gzip,
    Bzip2,
    Xz,
}

impl Algorithm {
    fn from_magic(magic: &[u8]) -> Option<Self> {
        if magic.starts_with(&GZIP_MAGIC) {
            Some(Self::Gzip)
        } else if magic.starts_with(&BZIP2_MAGIC) {
            Some(Self::Bzip2)
        } else if magic.starts_with(&XZ_MAGIC) {
            Some(Self::Xz)
        } else {
            None
        }
    }
}

#[pin_project]
struct WithPreexistingBuffer<R> {
    buffer: Vec<u8>,
    #[pin]
    inner: R,
}

impl<R> AsyncRead for WithPreexistingBuffer<R>
where
    R: AsyncRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.project();
        if !this.buffer.is_empty() {
            // TODO: check if the buffer fits first
            buf.put_slice(this.buffer);
            this.buffer.clear();
        }
        this.inner.poll_read(cx, buf)
    }
}

#[pin_project(project = DecompressedReaderInnerProj)]
enum DecompressedReaderInner<R> {
    Unknown {
        buffer: Vec<u8>,
        #[pin]
        inner: Option<R>,
    },
    Gzip(#[pin] GzipDecoder<BufReader<WithPreexistingBuffer<R>>>),
    Bzip2(#[pin] BzDecoder<BufReader<WithPreexistingBuffer<R>>>),
    Xz(#[pin] XzDecoder<BufReader<WithPreexistingBuffer<R>>>),
}

impl<R> DecompressedReaderInner<R>
where
    R: AsyncBufRead,
{
    fn switch_to(&mut self, algorithm: Algorithm) {
        let (buffer, inner) = match self {
            DecompressedReaderInner::Unknown { buffer, inner } => {
                (mem::take(buffer), inner.take().unwrap())
            }
            DecompressedReaderInner::Gzip(_)
            | DecompressedReaderInner::Bzip2(_)
            | DecompressedReaderInner::Xz(_) => unreachable!(),
        };
        let inner = BufReader::new(WithPreexistingBuffer { buffer, inner });

        *self = match algorithm {
            Algorithm::Gzip => Self::Gzip(GzipDecoder::new(inner)),
            Algorithm::Bzip2 => Self::Bzip2(BzDecoder::new(inner)),
            Algorithm::Xz => Self::Xz(XzDecoder::new(inner)),
        }
    }
}

impl<R> AsyncRead for DecompressedReaderInner<R>
where
    R: AsyncBufRead,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.project() {
            DecompressedReaderInnerProj::Unknown { .. } => {
                unreachable!("Can't call poll_read on Unknown")
            }
            DecompressedReaderInnerProj::Gzip(inner) => inner.poll_read(cx, buf),
            DecompressedReaderInnerProj::Bzip2(inner) => inner.poll_read(cx, buf),
            DecompressedReaderInnerProj::Xz(inner) => inner.poll_read(cx, buf),
        }
    }
}

#[pin_project]
pub struct DecompressedReader<R> {
    #[pin]
    inner: DecompressedReaderInner<R>,
    switch_to: Option<Algorithm>,
}

impl<R> DecompressedReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner: DecompressedReaderInner::Unknown {
                buffer: vec![0; BYTES_NEEDED],
                inner: Some(inner),
            },
            switch_to: None,
        }
    }
}

impl<R> AsyncRead for DecompressedReader<R>
where
    R: AsyncBufRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut this = self.project();
        let (buffer, inner) = match this.inner.as_mut().project() {
            DecompressedReaderInnerProj::Gzip(inner) => return inner.poll_read(cx, buf),
            DecompressedReaderInnerProj::Bzip2(inner) => return inner.poll_read(cx, buf),
            DecompressedReaderInnerProj::Xz(inner) => return inner.poll_read(cx, buf),
            DecompressedReaderInnerProj::Unknown { buffer, inner } => (buffer, inner),
        };

        let mut our_buf = ReadBuf::new(buffer);
        if let Err(e) = ready!(inner.as_pin_mut().unwrap().poll_read(cx, &mut our_buf)) {
            return Poll::Ready(Err(e));
        }

        let data = our_buf.filled();
        if data.len() >= BYTES_NEEDED {
            if let Some(algorithm) = Algorithm::from_magic(data) {
                this.inner.as_mut().switch_to(algorithm);
            } else {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "tar data not gz, bzip2, or xz compressed",
                )));
            }
            this.inner.poll_read(cx, buf)
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use async_compression::tokio::bufread::GzipEncoder;
    use futures::TryStreamExt;
    use rstest::rstest;
    use tokio::io::{AsyncReadExt, BufReader};
    use tokio_tar::Archive;

    use super::*;

    #[tokio::test]
    async fn gzip() {
        let data = b"abcdefghijk";
        let mut enc = GzipEncoder::new(&data[..]);
        let mut gzipped = vec![];
        enc.read_to_end(&mut gzipped).await.unwrap();

        let mut reader = DecompressedReader::new(BufReader::new(&gzipped[..]));
        let mut round_tripped = vec![];
        reader.read_to_end(&mut round_tripped).await.unwrap();

        assert_eq!(data[..], round_tripped[..]);
    }

    #[rstest]
    #[case::gzip(include_bytes!("../tests/blob.tar.gz"))]
    #[case::bzip2(include_bytes!("../tests/blob.tar.bz2"))]
    #[case::xz(include_bytes!("../tests/blob.tar.xz"))]
    #[tokio::test]
    async fn compressed_tar(#[case] data: &[u8]) {
        let reader = DecompressedReader::new(BufReader::new(data));
        let mut archive = Archive::new(reader);
        let mut entries: Vec<_> = archive.entries().unwrap().try_collect().await.unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path().unwrap().as_ref(), Path::new("empty"));
        let mut data = String::new();
        entries[0].read_to_string(&mut data).await.unwrap();
        assert_eq!(data, "");
    }
}
