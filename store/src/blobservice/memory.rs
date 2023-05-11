use data_encoding::BASE64;
use std::io::Cursor;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tracing::{instrument, warn};

use super::{BlobService, BlobWriter};
use crate::Error;

// type B3Digest = [u8; 32];
// struct B3Digest ([u8; 32]);

#[derive(Clone, Default)]
pub struct MemoryBlobService {
    db: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,
}

impl BlobService for MemoryBlobService {
    type BlobReader = Cursor<Vec<u8>>;
    type BlobWriter = MemoryBlobWriter;

    #[instrument(skip(self, digest), fields(blob.digest=BASE64.encode(digest)))]
    fn has(&self, digest: &[u8; 32]) -> Result<bool, Error> {
        let db = self.db.read().unwrap();
        Ok(db.contains_key(digest))
    }

    fn open_read(&self, digest: &[u8; 32]) -> Result<Option<Self::BlobReader>, Error> {
        let db = self.db.read().unwrap();

        Ok(db.get(digest).map(|x| Cursor::new(x.clone())))
    }

    #[instrument(skip(self))]
    fn open_write(&self) -> Result<Self::BlobWriter, Error> {
        Ok(MemoryBlobWriter::new(self.db.clone()))
    }
}

pub struct MemoryBlobWriter {
    db: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,

    buf: Vec<u8>,
}

impl MemoryBlobWriter {
    fn new(db: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>) -> Self {
        Self {
            buf: Vec::new(),
            db,
        }
    }
}
impl std::io::Write for MemoryBlobWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.buf.flush()
    }
}

impl BlobWriter for MemoryBlobWriter {
    fn close(self) -> Result<[u8; 32], Error> {
        // in this memory implementation, we don't actually bother hashing
        // incrementally while writing, but do it at the end.
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.buf);
        let digest: [u8; 32] = hasher.finalize().into();

        // open the database for writing.
        let mut db = self.db.write()?;
        db.insert(digest, self.buf);

        Ok(digest)
    }
}
