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
    type BlobReader = Cursor<Vec<u8>>;
    type BlobWriter = SledBlobWriter;

    #[instrument(skip(self), fields(blob.digest=%digest))]
    fn has(&self, digest: &B3Digest) -> Result<bool, Error> {
        match self.db.contains_key(digest.to_vec()) {
            Ok(has) => Ok(has),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip(self), fields(blob.digest=%digest))]
    fn open_read(&self, digest: &B3Digest) -> Result<Option<Self::BlobReader>, Error> {
        match self.db.get(digest.to_vec()) {
            Ok(None) => Ok(None),
            Ok(Some(data)) => Ok(Some(Cursor::new(data[..].to_vec()))),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip(self))]
    fn open_write(&self) -> Result<Self::BlobWriter, Error> {
        Ok(SledBlobWriter::new(self.db.clone()))
    }
}

pub struct SledBlobWriter {
    db: sled::Db,
    buf: Vec<u8>,
    hasher: blake3::Hasher,
}

impl SledBlobWriter {
    pub fn new(db: sled::Db) -> Self {
        Self {
            buf: Vec::default(),
            db,
            hasher: blake3::Hasher::new(),
        }
    }
}

impl io::Write for SledBlobWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_written = self.buf.write(buf)?;
        self.hasher.write(&buf[..bytes_written])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.buf.flush()
    }
}

impl BlobWriter for SledBlobWriter {
    fn close(self) -> Result<B3Digest, Error> {
        let digest = self.hasher.finalize();
        self.db
            .insert(digest.as_bytes(), self.buf)
            .map_err(|e| Error::StorageError(format!("unable to insert blob: {}", e)))?;

        // We know self.hasher is doing blake3 hashing, so this won't fail.
        Ok(B3Digest::from_vec(digest.as_bytes().to_vec()).unwrap())
    }
}
