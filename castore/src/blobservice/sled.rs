use super::{BlobReader, BlobService, BlobWriter};
use crate::{B3Digest, Error};
use std::{
    io::{self, Cursor, Write},
    path::Path,
    task::Poll,
};
use tonic::async_trait;
use tracing::instrument;

#[derive(Clone)]
pub struct SledBlobService {
    db: sled::Db,
}

impl SledBlobService {
    pub fn new<P: AsRef<Path>>(p: P) -> Result<Self, sled::Error> {
        let config = sled::Config::default()
            .use_compression(false) // is a required parameter
            .path(p);
        let db = config.open()?;

        Ok(Self { db })
    }

    pub fn new_temporary() -> Result<Self, sled::Error> {
        let config = sled::Config::default().temporary(true);
        let db = config.open()?;

        Ok(Self { db })
    }
}

#[async_trait]
impl BlobService for SledBlobService {
    #[instrument(skip(self), fields(blob.digest=%digest))]
    async fn has(&self, digest: &B3Digest) -> Result<bool, Error> {
        match self.db.contains_key(digest.as_slice()) {
            Ok(has) => Ok(has),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip(self), fields(blob.digest=%digest))]
    async fn open_read(&self, digest: &B3Digest) -> Result<Option<Box<dyn BlobReader>>, Error> {
        match self.db.get(digest.as_slice()) {
            Ok(None) => Ok(None),
            Ok(Some(data)) => Ok(Some(Box::new(Cursor::new(data[..].to_vec())))),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip(self))]
    async fn open_write(&self) -> Box<dyn BlobWriter> {
        Box::new(SledBlobWriter::new(self.db.clone()))
    }
}

pub struct SledBlobWriter {
    db: sled::Db,

    /// Contains the buffer Vec and hasher, or None if already closed
    writers: Option<(Vec<u8>, blake3::Hasher)>,

    /// The digest that has been returned, if we successfully closed.
    digest: Option<B3Digest>,
}

impl SledBlobWriter {
    pub fn new(db: sled::Db) -> Self {
        Self {
            db,
            writers: Some((Vec::new(), blake3::Hasher::new())),
            digest: None,
        }
    }
}

impl tokio::io::AsyncWrite for SledBlobWriter {
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
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        Poll::Ready(match &mut self.writers {
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
        // shutdown is "instantaneous", we only write to a Vec<u8> as buffer.
        Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl BlobWriter for SledBlobWriter {
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

            let digest: B3Digest = hasher.finalize().as_bytes().into();

            // Only insert if the blob doesn't already exist.
            if !self.db.contains_key(digest.as_slice()).map_err(|e| {
                Error::StorageError(format!("Unable to check if we have blob {}: {}", digest, e))
            })? {
                // put buf in there. This will move buf out.
                self.db
                    .insert(digest.as_slice(), buf)
                    .map_err(|e| Error::StorageError(format!("unable to insert blob: {}", e)))?;
            }

            self.digest = Some(digest.clone());

            Ok(digest)
        }
    }
}
