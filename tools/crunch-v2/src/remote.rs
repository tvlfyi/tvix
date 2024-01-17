use std::{
    cmp,
    io::{self, BufRead, BufReader, Read},
    pin::Pin,
    task::{self, Poll},
};

use anyhow::{bail, Result};
use bytes::{Buf, Bytes};
use futures::{future::BoxFuture, Future, FutureExt, Stream, StreamExt};
use lazy_static::lazy_static;
use tokio::runtime::Handle;

use nix_compat::nixbase32;

use rusoto_core::{ByteStream, Region};
use rusoto_s3::{GetObjectOutput, GetObjectRequest, S3Client, S3};

use bzip2::read::BzDecoder;
use xz2::read::XzDecoder;

lazy_static! {
    static ref S3_CLIENT: S3Client = S3Client::new(Region::UsEast1);
}

const BUCKET: &str = "nix-cache";

pub async fn nar(
    file_hash: [u8; 32],
    compression: &str,
) -> Result<Box<BufReader<dyn Read + Send>>> {
    let (extension, decompress): (&'static str, fn(_) -> Box<_>) = match compression {
        "bzip2" => ("bz2", decompress_bz2),
        "xz" => ("xz", decompress_xz),
        _ => bail!("unknown compression: {compression}"),
    };

    Ok(decompress(
        FileStream::new(FileKey {
            file_hash,
            extension,
        })
        .await?
        .into(),
    ))
}

fn decompress_xz(reader: FileStreamReader) -> Box<BufReader<dyn Read + Send>> {
    Box::new(BufReader::new(XzDecoder::new(reader)))
}

fn decompress_bz2(reader: FileStreamReader) -> Box<BufReader<dyn Read + Send>> {
    Box::new(BufReader::new(BzDecoder::new(reader)))
}

struct FileStreamReader {
    inner: FileStream,
    buffer: Bytes,
}

impl From<FileStream> for FileStreamReader {
    fn from(value: FileStream) -> Self {
        FileStreamReader {
            inner: value,
            buffer: Bytes::new(),
        }
    }
}

impl Read for FileStreamReader {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        let src = self.fill_buf()?;
        let n = cmp::min(src.len(), dst.len());
        dst[..n].copy_from_slice(&src[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl BufRead for FileStreamReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if !self.buffer.is_empty() {
            return Ok(&self.buffer);
        }

        self.buffer = Handle::current()
            .block_on(self.inner.next())
            .transpose()?
            .unwrap_or_default();

        Ok(&self.buffer)
    }

    fn consume(&mut self, cnt: usize) {
        self.buffer.advance(cnt);
    }
}

struct FileKey {
    file_hash: [u8; 32],
    extension: &'static str,
}

impl FileKey {
    fn get(
        &self,
        offset: u64,
        e_tag: Option<&str>,
    ) -> impl Future<Output = io::Result<GetObjectOutput>> + Send + 'static {
        let input = GetObjectRequest {
            bucket: BUCKET.to_string(),
            key: format!(
                "nar/{}.nar.{}",
                nixbase32::encode(&self.file_hash),
                self.extension
            ),
            if_match: e_tag.map(str::to_owned),
            range: Some(format!("bytes {}-", offset + 1)),
            ..Default::default()
        };

        async {
            S3_CLIENT
                .get_object(input)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        }
    }
}

struct FileStream {
    key: FileKey,
    e_tag: String,
    offset: u64,
    length: u64,
    inner: FileStreamState,
}

enum FileStreamState {
    Response(BoxFuture<'static, io::Result<GetObjectOutput>>),
    Body(ByteStream),
    Eof,
}

impl FileStream {
    pub async fn new(key: FileKey) -> io::Result<Self> {
        let resp = key.get(0, None).await?;

        Ok(FileStream {
            key,
            e_tag: resp.e_tag.unwrap(),
            offset: 0,
            length: resp.content_length.unwrap().try_into().unwrap(),
            inner: FileStreamState::Body(resp.body.unwrap()),
        })
    }
}

macro_rules! poll {
    ($expr:expr) => {
        match $expr {
            Poll::Pending => {
                return Poll::Pending;
            }
            Poll::Ready(value) => value,
        }
    };
}

impl Stream for FileStream {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        let chunk = loop {
            match &mut this.inner {
                FileStreamState::Response(resp) => match poll!(resp.poll_unpin(cx)) {
                    Err(err) => {
                        this.inner = FileStreamState::Eof;
                        return Poll::Ready(Some(Err(err)));
                    }
                    Ok(resp) => {
                        this.inner = FileStreamState::Body(resp.body.unwrap());
                    }
                },
                FileStreamState::Body(body) => match poll!(body.poll_next_unpin(cx)) {
                    None | Some(Err(_)) => {
                        this.inner = FileStreamState::Response(
                            this.key.get(this.offset, Some(&this.e_tag)).boxed(),
                        );
                    }
                    Some(Ok(chunk)) => {
                        break chunk;
                    }
                },
                FileStreamState::Eof => {
                    return Poll::Ready(None);
                }
            }
        };

        this.offset += chunk.len() as u64;

        if this.offset >= this.length {
            this.inner = FileStreamState::Eof;
        }

        Poll::Ready(Some(Ok(chunk)))
    }
}
