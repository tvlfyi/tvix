use super::{BlobService, BlobWriter};
use crate::{B3Digest, Error};
use std::{
    io::{self, Cursor},
    path::PathBuf,
};
use tracing::instrument;

#[derive(Clone)]
pub struct SledBlobService {
    db: sled::Db,
}

impl SledBlobService {
    pub fn new(p: PathBuf) -> Result<Self, sled::Error> {
        let config = sled::Config::default().use_compression(true).path(p);
        let db = config.open()?;

        Ok(Self { db })
    }

    pub fn new_temporary() -> Result<Self, sled::Error> {
        let config = sled::Config::default().temporary(true);
        let db = config.open()?;

        Ok(Self { db })
    }
}

impl BlobService for SledBlobService {
    #[instrument(skip(self), fields(blob.digest=%digest))]
    fn has(&self, digest: &B3Digest) -> Result<bool, Error> {
        match self.db.contains_key(digest.to_vec()) {
            Ok(has) => Ok(has),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip(self), fields(blob.digest=%digest))]
    fn open_read(&self, digest: &B3Digest) -> Result<Option<Box<dyn io::Read + Send>>, Error> {
        match self.db.get(digest.to_vec()) {
            Ok(None) => Ok(None),
            Ok(Some(data)) => Ok(Some(Box::new(Cursor::new(data[..].to_vec())))),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip(self))]
    fn open_write(&self) -> Result<Box<dyn BlobWriter>, Error> {
        Ok(Box::new(SledBlobWriter::new(self.db.clone())))
    }
}

pub struct SledBlobWriter {
    db: sled::Db,

    /// Contains the Vec and hasher, or None if already closed
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

impl io::Write for SledBlobWriter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match &mut self.writers {
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "already closed",
            )),
            Some((ref mut buf, ref mut hasher)) => {
                let bytes_written = buf.write(b)?;
                hasher.write(&buf[..bytes_written])
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.writers {
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "already closed",
            )),
            Some(_) => Ok(()),
        }
    }
}

impl BlobWriter for SledBlobWriter {
    fn close(&mut self) -> Result<B3Digest, Error> {
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
            let digest = B3Digest::from_vec(hasher.finalize().as_bytes().to_vec()).unwrap();

            // Only insert if the blob doesn't already exist.
            if !self.db.contains_key(digest.to_vec()).map_err(|e| {
                Error::StorageError(format!("Unable to check if we have blob {}: {}", digest, e))
            })? {
                // put buf in there. This will move buf out.
                self.db
                    .insert(digest.to_vec(), buf)
                    .map_err(|e| Error::StorageError(format!("unable to insert blob: {}", e)))?;
            }

            self.digest = Some(digest.clone());

            Ok(digest)
        }
    }
}
