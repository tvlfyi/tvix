use std::{
    fmt::Debug,
    future::Future,
    marker::PhantomData,
    ops::Deref,
    pin::Pin,
    task::{self, ready, Poll},
};

use tokio::io::{self, AsyncRead, ReadBuf};

/// Trailer represents up to 7 bytes of data read as part of the trailer block(s)
#[derive(Debug)]
pub(crate) struct Trailer {
    data_len: u8,
    buf: [u8; 7],
}

impl Deref for Trailer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.buf[..self.data_len as usize]
    }
}

/// Tag defines a "trailer tag": specific, fixed bytes that must follow wire data.
pub(crate) trait Tag {
    /// The expected suffix
    ///
    /// The first 7 bytes may be ignored, and it must be an 8-byte aligned size.
    const PATTERN: &'static [u8];

    /// Suitably sized buffer for reading [Self::PATTERN]
    ///
    /// HACK: This is a workaround for const generics limitations.
    type Buf: AsRef<[u8]> + AsMut<[u8]> + Debug + Unpin;

    /// Make an instance of [Self::Buf]
    fn make_buf() -> Self::Buf;
}

#[derive(Debug)]
pub enum Pad {}

impl Tag for Pad {
    const PATTERN: &'static [u8] = &[0; 8];

    type Buf = [u8; 8];

    fn make_buf() -> Self::Buf {
        [0; 8]
    }
}

#[derive(Debug)]
pub(crate) struct ReadTrailer<R, T: Tag> {
    reader: R,
    data_len: u8,
    filled: u8,
    buf: T::Buf,
    _phantom: PhantomData<fn(T) -> T>,
}

/// read_trailer returns a [Future] that reads a trailer with a given [Tag] from `reader`
pub(crate) fn read_trailer<R: AsyncRead + Unpin, T: Tag>(
    reader: R,
    data_len: u8,
) -> ReadTrailer<R, T> {
    assert!(data_len < 8, "payload in trailer must be less than 8 bytes");

    let buf = T::make_buf();
    assert_eq!(buf.as_ref().len(), T::PATTERN.len());
    assert_eq!(T::PATTERN.len() % 8, 0);

    ReadTrailer {
        reader,
        data_len,
        filled: if data_len != 0 { 0 } else { 8 },
        buf,
        _phantom: PhantomData,
    }
}

impl<R, T: Tag> ReadTrailer<R, T> {
    pub fn len(&self) -> u8 {
        self.data_len
    }
}

impl<R: AsyncRead + Unpin, T: Tag> Future for ReadTrailer<R, T> {
    type Output = io::Result<Trailer>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context) -> Poll<Self::Output> {
        let this = &mut *self;

        loop {
            if this.filled >= this.data_len {
                let check_range = || this.data_len as usize..this.filled as usize;

                if this.buf.as_ref()[check_range()] != T::PATTERN[check_range()] {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid trailer",
                    ))
                    .into();
                }
            }

            if this.filled as usize == T::PATTERN.len() {
                let mut buf = [0; 7];
                buf.copy_from_slice(&this.buf.as_ref()[..7]);

                return Ok(Trailer {
                    data_len: this.data_len,
                    buf,
                })
                .into();
            }

            let mut buf = ReadBuf::new(this.buf.as_mut());
            buf.advance(this.filled as usize);

            ready!(Pin::new(&mut this.reader).poll_read(cx, &mut buf))?;

            this.filled = {
                let filled = buf.filled().len() as u8;

                if filled == this.filled {
                    return Err(io::ErrorKind::UnexpectedEof.into()).into();
                }

                filled
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn unexpected_eof() {
        let reader = tokio_test::io::Builder::new()
            .read(&[0xed])
            .wait(Duration::ZERO)
            .read(&[0xef, 0x00])
            .build();

        assert_eq!(
            read_trailer::<_, Pad>(reader, 2).await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn invalid_padding() {
        let reader = tokio_test::io::Builder::new()
            .read(&[0xed])
            .wait(Duration::ZERO)
            .read(&[0xef, 0x01, 0x00])
            .wait(Duration::ZERO)
            .build();

        assert_eq!(
            read_trailer::<_, Pad>(reader, 2).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[tokio::test]
    async fn success() {
        let reader = tokio_test::io::Builder::new()
            .read(&[0xed])
            .wait(Duration::ZERO)
            .read(&[0xef, 0x00])
            .wait(Duration::ZERO)
            .read(&[0x00, 0x00, 0x00, 0x00, 0x00])
            .build();

        assert_eq!(
            &*read_trailer::<_, Pad>(reader, 2).await.unwrap(),
            &[0xed, 0xef]
        );
    }

    #[tokio::test]
    async fn no_padding() {
        assert!(read_trailer::<_, Pad>(io::empty(), 0)
            .await
            .unwrap()
            .is_empty());
    }
}
