use std::future::poll_fn;
use std::io::{self, Cursor};
use std::ops::RangeInclusive;
use std::pin::Pin;
use std::task::{ready, Context, Poll};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use pin_project_lite::pin_project;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncReadExt, ReadBuf};

use crate::nix_daemon::ProtocolVersion;
use crate::wire::EMPTY_BYTES;

use super::{Error, NixRead};

pub struct NixReaderBuilder {
    buf: Option<BytesMut>,
    reserved_buf_size: usize,
    max_buf_size: usize,
    version: ProtocolVersion,
}

impl Default for NixReaderBuilder {
    fn default() -> Self {
        Self {
            buf: Default::default(),
            reserved_buf_size: 8192,
            max_buf_size: 8192,
            version: Default::default(),
        }
    }
}

impl NixReaderBuilder {
    pub fn set_buffer(mut self, buf: BytesMut) -> Self {
        self.buf = Some(buf);
        self
    }

    pub fn set_reserved_buf_size(mut self, size: usize) -> Self {
        self.reserved_buf_size = size;
        self
    }

    pub fn set_max_buf_size(mut self, size: usize) -> Self {
        self.max_buf_size = size;
        self
    }

    pub fn set_version(mut self, version: ProtocolVersion) -> Self {
        self.version = version;
        self
    }

    pub fn build<R>(self, reader: R) -> NixReader<R> {
        let buf = self.buf.unwrap_or_else(|| BytesMut::with_capacity(0));
        NixReader {
            buf,
            inner: reader,
            reserved_buf_size: self.reserved_buf_size,
            max_buf_size: self.max_buf_size,
            version: self.version,
        }
    }
}

pin_project! {
    pub struct NixReader<R> {
        #[pin]
        inner: R,
        buf: BytesMut,
        reserved_buf_size: usize,
        max_buf_size: usize,
        version: ProtocolVersion,
    }
}

impl NixReader<Cursor<Vec<u8>>> {
    pub fn builder() -> NixReaderBuilder {
        NixReaderBuilder::default()
    }
}

impl<R> NixReader<R>
where
    R: AsyncReadExt,
{
    pub fn new(reader: R) -> NixReader<R> {
        NixReader::builder().build(reader)
    }

    pub fn buffer(&self) -> &[u8] {
        &self.buf[..]
    }

    #[cfg(test)]
    pub(crate) fn buffer_mut(&mut self) -> &mut BytesMut {
        &mut self.buf
    }

    /// Remaining capacity in internal buffer
    pub fn remaining_mut(&self) -> usize {
        self.buf.capacity() - self.buf.len()
    }

    fn poll_force_fill_buf(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<usize>> {
        // Ensure that buffer has space for at least reserved_buf_size bytes
        if self.remaining_mut() < self.reserved_buf_size {
            let me = self.as_mut().project();
            me.buf.reserve(*me.reserved_buf_size);
        }
        let me = self.project();
        let n = {
            let dst = me.buf.spare_capacity_mut();
            let mut buf = ReadBuf::uninit(dst);
            let ptr = buf.filled().as_ptr();
            ready!(me.inner.poll_read(cx, &mut buf)?);

            // Ensure the pointer does not change from under us
            assert_eq!(ptr, buf.filled().as_ptr());
            buf.filled().len()
        };

        // SAFETY: This is guaranteed to be the number of initialized (and read)
        // bytes due to the invariants provided by `ReadBuf::filled`.
        unsafe {
            me.buf.advance_mut(n);
        }
        Poll::Ready(Ok(n))
    }
}

impl<R> NixReader<R>
where
    R: AsyncReadExt + Unpin,
{
    async fn force_fill(&mut self) -> io::Result<usize> {
        let mut p = Pin::new(self);
        let read = poll_fn(|cx| p.as_mut().poll_force_fill_buf(cx)).await?;
        Ok(read)
    }
}

impl<R> NixRead for NixReader<R>
where
    R: AsyncReadExt + Send + Unpin,
{
    type Error = io::Error;

    fn version(&self) -> ProtocolVersion {
        self.version
    }

    async fn try_read_number(&mut self) -> Result<Option<u64>, Self::Error> {
        let mut buf = [0u8; 8];
        let read = self.read_buf(&mut &mut buf[..]).await?;
        if read == 0 {
            return Ok(None);
        }
        if read < 8 {
            self.read_exact(&mut buf[read..]).await?;
        }
        let num = Buf::get_u64_le(&mut &buf[..]);
        Ok(Some(num))
    }

    async fn try_read_bytes_limited(
        &mut self,
        limit: RangeInclusive<usize>,
    ) -> Result<Option<Bytes>, Self::Error> {
        assert!(
            *limit.end() <= self.max_buf_size,
            "The limit must be smaller than {}",
            self.max_buf_size
        );
        match self.try_read_number().await? {
            Some(raw_len) => {
                // Check that length is in range and convert to usize
                let len = raw_len
                    .try_into()
                    .ok()
                    .filter(|v| limit.contains(v))
                    .ok_or_else(|| Self::Error::invalid_data("bytes length out of range"))?;

                // Calculate 64bit aligned length and convert to usize
                let aligned: usize = raw_len
                    .checked_add(7)
                    .map(|v| v & !7)
                    .ok_or_else(|| Self::Error::invalid_data("bytes length out of range"))?
                    .try_into()
                    .map_err(Self::Error::invalid_data)?;

                // Ensure that there is enough space in buffer for contents
                if self.buf.len() + self.remaining_mut() < aligned {
                    self.buf.reserve(aligned - self.buf.len());
                }
                while self.buf.len() < aligned {
                    if self.force_fill().await? == 0 {
                        return Err(Self::Error::missing_data(
                            "unexpected end-of-file reading bytes",
                        ));
                    }
                }
                let mut contents = self.buf.split_to(aligned);

                let padding = aligned - len;
                // Ensure padding is all zeros
                if contents[len..] != EMPTY_BYTES[..padding] {
                    return Err(Self::Error::invalid_data("non-zero padding"));
                }

                contents.truncate(len);
                Ok(Some(contents.freeze()))
            }
            None => Ok(None),
        }
    }

    fn try_read_bytes(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Option<Bytes>, Self::Error>> + Send + '_ {
        self.try_read_bytes_limited(0..=self.max_buf_size)
    }

    fn read_bytes(
        &mut self,
    ) -> impl std::future::Future<Output = Result<Bytes, Self::Error>> + Send + '_ {
        self.read_bytes_limited(0..=self.max_buf_size)
    }
}

impl<R: AsyncRead> AsyncRead for NixReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let rem = ready!(self.as_mut().poll_fill_buf(cx))?;
        let amt = std::cmp::min(rem.len(), buf.remaining());
        buf.put_slice(&rem[0..amt]);
        self.consume(amt);
        Poll::Ready(Ok(()))
    }
}

impl<R: AsyncRead> AsyncBufRead for NixReader<R> {
    fn poll_fill_buf(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        if self.as_ref().project_ref().buf.is_empty() {
            ready!(self.as_mut().poll_force_fill_buf(cx))?;
        }
        let me = self.project();
        Poll::Ready(Ok(&me.buf[..]))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let me = self.project();
        me.buf.advance(amt)
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use hex_literal::hex;
    use rstest::rstest;
    use tokio_test::io::Builder;

    use super::*;
    use crate::nix_daemon::de::NixRead;

    #[tokio::test]
    async fn test_read_u64() {
        let mock = Builder::new().read(&hex!("0100 0000 0000 0000")).build();
        let mut reader = NixReader::new(mock);

        assert_eq!(1, reader.read_number().await.unwrap());
        assert_eq!(hex!(""), reader.buffer());

        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(hex!(""), &buf[..]);
    }

    #[tokio::test]
    async fn test_read_u64_rest() {
        let mock = Builder::new()
            .read(&hex!("0100 0000 0000 0000 0123 4567 89AB CDEF"))
            .build();
        let mut reader = NixReader::new(mock);

        assert_eq!(1, reader.read_number().await.unwrap());
        assert_eq!(hex!("0123 4567 89AB CDEF"), reader.buffer());

        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(hex!("0123 4567 89AB CDEF"), &buf[..]);
    }

    #[tokio::test]
    async fn test_read_u64_partial() {
        let mock = Builder::new()
            .read(&hex!("0100 0000"))
            .wait(Duration::ZERO)
            .read(&hex!("0000 0000 0123 4567 89AB CDEF"))
            .wait(Duration::ZERO)
            .read(&hex!("0100 0000"))
            .build();
        let mut reader = NixReader::new(mock);

        assert_eq!(1, reader.read_number().await.unwrap());
        assert_eq!(hex!("0123 4567 89AB CDEF"), reader.buffer());

        let mut buf = Vec::new();
        reader.read_to_end(&mut buf).await.unwrap();
        assert_eq!(hex!("0123 4567 89AB CDEF 0100 0000"), &buf[..]);
    }

    #[tokio::test]
    async fn test_read_u64_eof() {
        let mock = Builder::new().build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::UnexpectedEof,
            reader.read_number().await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_try_read_u64_none() {
        let mock = Builder::new().build();
        let mut reader = NixReader::new(mock);

        assert_eq!(None, reader.try_read_number().await.unwrap());
    }

    #[tokio::test]
    async fn test_try_read_u64_eof() {
        let mock = Builder::new().read(&hex!("0100 0000 0000")).build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::UnexpectedEof,
            reader.try_read_number().await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_try_read_u64_eof2() {
        let mock = Builder::new()
            .read(&hex!("0100"))
            .wait(Duration::ZERO)
            .read(&hex!("0000 0000"))
            .build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::UnexpectedEof,
            reader.try_read_number().await.unwrap_err().kind()
        );
    }

    #[rstest]
    #[case::empty(b"", &hex!("0000 0000 0000 0000"))]
    #[case::one(b")", &hex!("0100 0000 0000 0000 2900 0000 0000 0000"))]
    #[case::two(b"it", &hex!("0200 0000 0000 0000 6974 0000 0000 0000"))]
    #[case::three(b"tea", &hex!("0300 0000 0000 0000 7465 6100 0000 0000"))]
    #[case::four(b"were", &hex!("0400 0000 0000 0000 7765 7265 0000 0000"))]
    #[case::five(b"where", &hex!("0500 0000 0000 0000 7768 6572 6500 0000"))]
    #[case::six(b"unwrap", &hex!("0600 0000 0000 0000 756E 7772 6170 0000"))]
    #[case::seven(b"where's", &hex!("0700 0000 0000 0000 7768 6572 6527 7300"))]
    #[case::aligned(b"read_tea", &hex!("0800 0000 0000 0000 7265 6164 5F74 6561"))]
    #[case::more_bytes(b"read_tess", &hex!("0900 0000 0000 0000 7265 6164 5F74 6573 7300 0000 0000 0000"))]
    #[tokio::test]
    async fn test_read_bytes(#[case] expected: &[u8], #[case] data: &[u8]) {
        let mock = Builder::new().read(data).build();
        let mut reader = NixReader::new(mock);
        let actual = reader.read_bytes().await.unwrap();
        assert_eq!(&actual[..], expected);
    }

    #[tokio::test]
    async fn test_read_bytes_empty() {
        let mock = Builder::new().build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::UnexpectedEof,
            reader.read_bytes().await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_try_read_bytes_none() {
        let mock = Builder::new().build();
        let mut reader = NixReader::new(mock);

        assert_eq!(None, reader.try_read_bytes().await.unwrap());
    }

    #[tokio::test]
    async fn test_try_read_bytes_missing_data() {
        let mock = Builder::new()
            .read(&hex!("0500"))
            .wait(Duration::ZERO)
            .read(&hex!("0000 0000"))
            .build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::UnexpectedEof,
            reader.try_read_bytes().await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_try_read_bytes_missing_padding() {
        let mock = Builder::new()
            .read(&hex!("0200 0000 0000 0000"))
            .wait(Duration::ZERO)
            .read(&hex!("1234"))
            .build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::UnexpectedEof,
            reader.try_read_bytes().await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_read_bytes_bad_padding() {
        let mock = Builder::new()
            .read(&hex!("0200 0000 0000 0000"))
            .wait(Duration::ZERO)
            .read(&hex!("1234 0100 0000 0000"))
            .build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::InvalidData,
            reader.read_bytes().await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_read_bytes_limited_out_of_range() {
        let mock = Builder::new().read(&hex!("FFFF 0000 0000 0000")).build();
        let mut reader = NixReader::new(mock);

        assert_eq!(
            io::ErrorKind::InvalidData,
            reader.read_bytes_limited(0..=50).await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_read_bytes_length_overflow() {
        let mock = Builder::new().read(&hex!("F9FF FFFF FFFF FFFF")).build();
        let mut reader = NixReader::builder()
            .set_max_buf_size(usize::MAX)
            .build(mock);

        assert_eq!(
            io::ErrorKind::InvalidData,
            reader
                .read_bytes_limited(0..=usize::MAX)
                .await
                .unwrap_err()
                .kind()
        );
    }

    // FUTUREWORK: Test this on supported hardware
    #[tokio::test]
    #[cfg(any(target_pointer_width = "16", target_pointer_width = "32"))]
    async fn test_bytes_length_conversion_overflow() {
        let len = (usize::MAX as u64) + 1;
        let mock = Builder::new().read(&len.to_le_bytes()).build();
        let mut reader = NixReader::new(mock);
        assert_eq!(
            std::io::ErrorKind::InvalidData,
            reader.read_value::<usize>().await.unwrap_err().kind()
        );
    }

    // FUTUREWORK: Test this on supported hardware
    #[tokio::test]
    #[cfg(any(target_pointer_width = "16", target_pointer_width = "32"))]
    async fn test_bytes_aligned_length_conversion_overflow() {
        let len = (usize::MAX - 6) as u64;
        let mock = Builder::new().read(&len.to_le_bytes()).build();
        let mut reader = NixReader::new(mock);
        assert_eq!(
            std::io::ErrorKind::InvalidData,
            reader.read_value::<usize>().await.unwrap_err().kind()
        );
    }

    #[tokio::test]
    async fn test_buffer_resize() {
        let mock = Builder::new()
            .read(&hex!("0100"))
            .read(&hex!("0000 0000 0000"))
            .build();
        let mut reader = NixReader::builder().set_reserved_buf_size(8).build(mock);
        // buffer has no capacity initially
        assert_eq!(0, reader.buffer_mut().capacity());

        assert_eq!(2, reader.force_fill().await.unwrap());

        // After first read buffer should have capacity we chose
        assert_eq!(8, reader.buffer_mut().capacity());

        // Because there was only 6 bytes remaining in buffer,
        // which is enough to read the last 6 bytes, but we require
        // capacity for 8 bytes, it doubled the capacity
        assert_eq!(6, reader.force_fill().await.unwrap());
        assert_eq!(16, reader.buffer_mut().capacity());

        assert_eq!(1, reader.read_number().await.unwrap());
    }
}
