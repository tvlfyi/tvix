use std::{
    pin::Pin,
    task::{self, ready, Poll},
};

use tokio::io::{self, AsyncRead, ReadBuf};

#[derive(Debug)]
pub enum TrailerReader<R> {
    Reading {
        reader: R,
        user_len: u8,
        filled: u8,
        buf: [u8; 8],
    },
    Releasing {
        off: u8,
        len: u8,
        buf: [u8; 8],
    },
    Done,
}

impl<R: AsyncRead + Unpin> TrailerReader<R> {
    pub fn new(reader: R, user_len: u8) -> Self {
        if user_len == 0 {
            return Self::Done;
        }

        assert!(user_len < 8, "payload in trailer must be less than 8 bytes");
        Self::Reading {
            reader,
            user_len,
            filled: 0,
            buf: [0; 8],
        }
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
                &mut Self::Reading {
                    reader: _,
                    user_len,
                    filled: 8,
                    buf,
                } => {
                    *this = Self::Releasing {
                        off: 0,
                        len: user_len,
                        buf,
                    };
                }
                Self::Reading {
                    reader,
                    user_len,
                    filled,
                    buf,
                } => {
                    let mut read_buf = ReadBuf::new(&mut buf[..]);
                    read_buf.advance(*filled as usize);
                    ready!(Pin::new(reader).poll_read(cx, &mut read_buf))?;

                    let new_filled = read_buf.filled().len() as u8;
                    if *filled == new_filled {
                        return Err(io::ErrorKind::UnexpectedEof.into()).into();
                    }

                    *filled = new_filled;

                    // ensure the padding is all zeroes
                    if (u64::from_le_bytes(*buf) >> (*user_len * 8)) != 0 {
                        return Err(io::ErrorKind::InvalidData.into()).into();
                    }
                }
                Self::Releasing { off: 8, .. } => {
                    *this = Self::Done;
                }
                Self::Releasing { off, len, buf } => {
                    assert_ne!(user_buf.remaining(), 0);

                    let buf = &buf[*off as usize..*len as usize];
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
