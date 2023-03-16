use std::path::PathBuf;

use data_encoding::BASE64;
use tracing::instrument;

use crate::Error;

use super::ChunkService;

#[derive(Clone)]
pub struct SledChunkService {
    db: sled::Db,
}

impl SledChunkService {
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

impl ChunkService for SledChunkService {
    #[instrument(name = "SledChunkService::has", skip(self, digest), fields(chunk.digest=BASE64.encode(digest)))]
    fn has(&self, digest: &[u8; 32]) -> Result<bool, Error> {
        match self.db.get(digest) {
            Ok(None) => Ok(false),
            Ok(Some(_)) => Ok(true),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(name = "SledChunkService::get", skip(self), fields(chunk.digest=BASE64.encode(digest)))]
    fn get(&self, digest: &[u8; 32]) -> Result<Option<Vec<u8>>, Error> {
        match self.db.get(digest) {
            Ok(None) => Ok(None),
            Ok(Some(data)) => {
                // calculate the hash to verify this is really what we expect
                let actual_digest = blake3::hash(&data).as_bytes().to_vec();
                if actual_digest != digest {
                    return Err(Error::StorageError(format!(
                        "invalid hash encountered when reading chunk, expected {}, got {}",
                        BASE64.encode(digest),
                        BASE64.encode(&actual_digest),
                    )));
                }
                Ok(Some(Vec::from(&*data)))
            }
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(name = "SledChunkService::put", skip(self, data))]
    fn put(&self, data: Vec<u8>) -> Result<[u8; 32], Error> {
        let digest = blake3::hash(&data);
        let result = self.db.insert(digest.as_bytes(), data);
        if let Err(e) = result {
            return Err(Error::StorageError(e.to_string()));
        }
        Ok(*digest.as_bytes())
    }
}
