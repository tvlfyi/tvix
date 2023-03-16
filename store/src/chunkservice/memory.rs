use data_encoding::BASE64;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tracing::instrument;

use crate::Error;

use super::ChunkService;

#[derive(Clone, Default)]
pub struct MemoryChunkService {
    db: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,
}

impl ChunkService for MemoryChunkService {
    #[instrument(skip(self, digest), fields(chunk.digest=BASE64.encode(digest)))]
    fn has(&self, digest: &[u8; 32]) -> Result<bool, Error> {
        let db = self.db.read().unwrap();
        Ok(db.get(digest).is_some())
    }

    #[instrument(skip(self), fields(chunk.digest=BASE64.encode(digest)))]
    fn get(&self, digest: &[u8; 32]) -> Result<Option<Vec<u8>>, Error> {
        let db = self.db.read().unwrap();
        match db.get(digest) {
            None => Ok(None),
            Some(data) => {
                // calculate the hash to verify this is really what we expect
                let actual_digest = blake3::hash(data).as_bytes().to_vec();
                if actual_digest != digest {
                    return Err(Error::StorageError(format!(
                        "invalid hash encountered when reading chunk, expected {}, got {}",
                        BASE64.encode(digest),
                        BASE64.encode(&actual_digest),
                    )));
                }
                Ok(Some(data.clone()))
            }
        }
    }

    #[instrument(skip(self, data))]
    fn put(&self, data: Vec<u8>) -> Result<[u8; 32], Error> {
        let digest = blake3::hash(&data);

        let mut db = self.db.write().unwrap();
        db.insert(*digest.as_bytes(), data);

        Ok(*digest.as_bytes())
    }
}
