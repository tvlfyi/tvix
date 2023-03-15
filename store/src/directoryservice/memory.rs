use crate::{proto, Error};
use data_encoding::BASE64;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{instrument, warn};

use super::DirectoryService;

#[derive(Clone, Default)]
pub struct MemoryDirectoryService {
    db: Arc<RwLock<HashMap<[u8; 32], proto::Directory>>>,
}

impl DirectoryService for MemoryDirectoryService {
    #[instrument(skip(self, digest), fields(directory.digest = BASE64.encode(digest)))]
    fn get(&self, digest: &[u8; 32]) -> Result<Option<proto::Directory>, Error> {
        let db = self.db.read()?;

        match db.get(digest) {
            // The directory was not found, return
            None => Ok(None),

            // The directory was found, try to parse the data as Directory message
            Some(directory) => {
                // Validate the retrieved Directory indeed has the
                // digest we expect it to have, to detect corruptions.
                let actual_digest = directory.digest();
                if actual_digest.as_slice() != digest {
                    return Err(Error::StorageError(format!(
                        "requested directory with digest {}, but got {}",
                        BASE64.encode(digest),
                        BASE64.encode(&actual_digest)
                    )));
                }

                Ok(Some(directory.clone()))
            }
        }
    }

    #[instrument(skip(self, directory), fields(directory.digest = BASE64.encode(&directory.digest())))]
    fn put(&self, directory: proto::Directory) -> Result<[u8; 32], Error> {
        let digest = directory.digest();

        // validate the directory itself.
        if let Err(e) = directory.validate() {
            return Err(Error::InvalidRequest(format!(
                "directory {} failed validation: {}",
                BASE64.encode(&digest),
                e,
            )));
        }

        // store it
        let mut db = self.db.write()?;
        db.insert(digest, directory);

        Ok(digest)
    }
}
