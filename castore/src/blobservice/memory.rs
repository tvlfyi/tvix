use std::io::{self, Cursor, Write};
use std::task::Poll;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tonic::async_trait;
use tracing::instrument;

use super::{BlobReader, BlobService, BlobWriter};
use crate::{B3Digest, Error};

#[derive(Clone, Default)]
pub struct MemoryBlobService {
    db: Arc<RwLock<HashMap<B3Digest, Vec<u8>>>>,
}

#[async_trait]
impl BlobService for MemoryBlobService {
    #[instrument(skip(self, digest), fields(blob.digest=%digest))]
    async fn has(&self, digest: &B3Digest) -> Result<bool, Error> {
        let db = self.db.read().unwrap();
        Ok(db.contains_key(digest))
    }

    async fn open_read(&self, digest: &B3Digest) -> Result<Option<Box<dyn BlobReader>>, Error> {
        let db = self.db.read().unwrap();

        match db.get(digest).map(|x| Cursor::new(x.clone())) {
            Some(result) => Ok(Some(Box::new(result))),
            None => Ok(None),
        }
    }

    #[instrument(skip(self))]
    async fn open_write(&self) -> Box<dyn BlobWriter> {
        Box::new(MemoryBlobWriter::new(self.db.clone()))
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
    async fn close(&mut self) -> Result<B3Digest, Error> {
        if self.writers.is_none() {
            match &self.digest {
                Some(digest) => Ok(digest.clone()),
                None => Err(crate::Error::StorageError(
                    "previously closed with error".to_string(),
                )),
            }
        } else {
            let (buf, hasher) = self.writers.take().unwrap();

            // We know self.hasher is doing blake3 hashing, so this won't fail.
            let digest: B3Digest = hasher.finalize().as_bytes().into();

            // Only insert if the blob doesn't already exist.
            let db = self.db.read()?;
            if !db.contains_key(&digest) {
                // drop the read lock, so we can open for writing.
                drop(db);

                // open the database for writing.
                let mut db = self.db.write()?;

                // and put buf in there. This will move buf out.
                db.insert(digest.clone(), buf);
            }

            self.digest = Some(digest.clone());

            Ok(digest)
        }
    }
}
