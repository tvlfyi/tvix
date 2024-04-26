use std::{
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
    type Buf: AsRef<[u8]> + AsMut<[u8]> + Unpin;

    /// Make an instance of [Self::Buf]
    fn make_buf() -> Self::Buf;
}

#[derive(Debug)]
pub(crate) enum Pad {}

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
    _phantom: PhantomData<*const T>,
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

impl<R: AsyncRead + Unpin, T: Tag> Future for ReadTrailer<R, T> {
    type Output = io::Result<Trailer>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context) -> task::Poll<Self::Output> {
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
                let prev_filled = this.filled;
                let filled = buf.filled().len() as u8;

                if filled == prev_filled {
                    return Err(io::ErrorKind::UnexpectedEof.into()).into();
                }

                filled
            };
        }
    }
}

#[derive(Debug)]
pub(crate) enum TrailerReader<R> {
    Reading(ReadTrailer<R, Pad>),
    Releasing { off: u8, data: Trailer },
    Done,
}

impl<R: AsyncRead + Unpin> TrailerReader<R> {
    pub fn new(reader: R, data_len: u8) -> Self {
        Self::Reading(read_trailer(reader, data_len))
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for TrailerReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context,
        user_buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let this = &mut *self;

        loop {
            match this {
                Self::Reading(fut) => {
                    *this = Self::Releasing {
                        off: 0,
                        data: ready!(Pin::new(fut).poll(cx))?,
                    };
                }
                Self::Releasing { off: 8, .. } => {
                    *this = Self::Done;
                }
                Self::Releasing { off, data } => {
                    assert_ne!(user_buf.remaining(), 0);

                    let buf = &data[*off as usize..];
                    let buf = &buf[..usize::min(buf.len(), user_buf.remaining())];

                    user_buf.put_slice(buf);
                    *off += buf.len() as u8;

                    break;
                }
                Self::Done => break,
            }
        }

        Ok(()).into()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tokio::io::AsyncReadExt;

    use super::*;

    #[tokio::test]
    async fn unexpected_eof() {
        let reader = tokio_test::io::Builder::new()
            .read(&[0xed])
            .wait(Duration::ZERO)
            .read(&[0xef, 0x00])
            .build();

        let mut reader = TrailerReader::new(reader, 2);

        let mut buf = vec![];
        assert_eq!(
            reader.read_to_end(&mut buf).await.unwrap_err().kind(),
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

        let mut reader = TrailerReader::new(reader, 2);

        let mut buf = vec![];
        assert_eq!(
            reader.read_to_end(&mut buf).await.unwrap_err().kind(),
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

        let mut reader = TrailerReader::new(reader, 2);

        let mut buf = vec![];
        reader.read_to_end(&mut buf).await.unwrap();

        assert_eq!(buf, &[0xed, 0xef]);
    }

    #[tokio::test]
    async fn no_padding() {
        let reader = tokio_test::io::Builder::new().build();
        let mut reader = TrailerReader::new(reader, 0);

        let mut buf = vec![];
        reader.read_to_end(&mut buf).await.unwrap();
        assert!(buf.is_empty());
    }
}
