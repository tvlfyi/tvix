use parking_lot::RwLock;
use std::io::{self, Cursor, Write};
use std::task::Poll;
use std::{collections::HashMap, sync::Arc};
use tonic::async_trait;
use tracing::instrument;

use super::{BlobReader, BlobService, BlobWriter};
use crate::composition::{CompositionContext, ServiceBuilder};
use crate::{B3Digest, Error};

#[derive(Clone, Default)]
pub struct MemoryBlobService {
    db: Arc<RwLock<HashMap<B3Digest, Vec<u8>>>>,
}

#[async_trait]
impl BlobService for MemoryBlobService {
    #[instrument(skip_all, ret, err, fields(blob.digest=%digest))]
    async fn has(&self, digest: &B3Digest) -> io::Result<bool> {
        let db = self.db.read();
        Ok(db.contains_key(digest))
    }

    #[instrument(skip_all, err, fields(blob.digest=%digest))]
    async fn open_read(&self, digest: &B3Digest) -> io::Result<Option<Box<dyn BlobReader>>> {
        let db = self.db.read();

        match db.get(digest).map(|x| Cursor::new(x.clone())) {
            Some(result) => Ok(Some(Box::new(result))),
            None => Ok(None),
        }
    }

    #[instrument(skip_all)]
    async fn open_write(&self) -> Box<dyn BlobWriter> {
        Box::new(MemoryBlobWriter::new(self.db.clone()))
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct MemoryBlobServiceConfig {}

impl TryFrom<url::Url> for MemoryBlobServiceConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        // memory doesn't support host or path in the URL.
        if url.has_host() || !url.path().is_empty() {
            return Err(Error::StorageError("invalid url".to_string()).into());
        }
        Ok(MemoryBlobServiceConfig {})
    }
}

#[async_trait]
impl ServiceBuilder for MemoryBlobServiceConfig {
    type Output = dyn BlobService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        _context: &CompositionContext<dyn BlobService>,
    ) -> Result<Arc<dyn BlobService>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(Arc::new(MemoryBlobService::default()))
    }
}

pub struct MemoryBlobWriter {
    db: Arc<RwLock<HashMap<B3Digest, Vec<u8>>>>,

    /// Contains the buffer Vec and hasher, or None if already closed
    writers: Option<(Vec<u8>, blake3::Hasher)>,

    /// The digest that has been returned, if we successfully closed.
    digest: Option<B3Digest>,
}

impl MemoryBlobWriter {
    fn new(db: Arc<RwLock<HashMap<B3Digest, Vec<u8>>>>) -> Self {
        Self {
            db,
            writers: Some((Vec::new(), blake3::Hasher::new())),
            digest: None,
        }
    }
}
impl tokio::io::AsyncWrite for MemoryBlobWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        b: &[u8],
    ) -> std::task::Poll<Result<usize, io::Error>> {
        Poll::Ready(match &mut self.writers {
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "already closed",
            )),
            Some((ref mut buf, ref mut hasher)) => {
                let bytes_written = buf.write(b)?;
                hasher.write(&b[..bytes_written])
            }
        })
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        Poll::Ready(match self.writers {
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "already closed",
            )),
            Some(_) => Ok(()),
        })
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        // shutdown is "instantaneous", we only write to memory.
        Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl BlobWriter for MemoryBlobWriter {
    async fn close(&mut self) -> io::Result<B3Digest> {
        if self.writers.is_none() {
            match &self.digest {
                Some(digest) => Ok(digest.clone()),
                None => Err(io::Error::new(io::ErrorKind::BrokenPipe, "already closed")),
            }
        } else {
            let (buf, hasher) = self.writers.take().unwrap();

            let digest: B3Digest = hasher.finalize().as_bytes().into();

            // Only insert if the blob doesn't already exist.
            let mut db = self.db.upgradable_read();
            if !db.contains_key(&digest) {
                // open the database for writing.
                db.with_upgraded(|db| {
                    // and put buf in there. This will move buf out.
                    db.insert(digest.clone(), buf);
                });
            }

            self.digest = Some(digest.clone());

            Ok(digest)
        }
    }
}
