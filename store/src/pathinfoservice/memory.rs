use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{proto, Error};
use nix_compat::store_path::DIGEST_SIZE;

use super::PathInfoService;

#[derive(Default)]
pub struct MemoryPathInfoService {
    db: Arc<RwLock<HashMap<Vec<u8>, proto::PathInfo>>>,
}

impl PathInfoService for MemoryPathInfoService {
    fn get(
        &self,
        by_what: proto::get_path_info_request::ByWhat,
    ) -> Result<Option<proto::PathInfo>, Error> {
        match by_what {
            proto::get_path_info_request::ByWhat::ByOutputHash(digest) => {
                if digest.len() != DIGEST_SIZE {
                    return Err(Error::InvalidRequest("invalid digest length".to_string()));
                }

                let db = self.db.read().unwrap();

                match db.get(&digest) {
                    None => Ok(None),
                    Some(path_info) => Ok(Some(path_info.clone())),
                }
            }
        }
    }

    fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, Error> {
        // Call validate on the received PathInfo message.
        match path_info.validate() {
            Err(e) => Err(Error::InvalidRequest(format!(
                "failed to validate PathInfo: {}",
                e
            ))),

            // In case the PathInfo is valid, and we were able to extract a NixPath, store it in the database.
            // This overwrites existing PathInfo objects.
            Ok(nix_path) => {
                let mut db = self.db.write().unwrap();
                db.insert(nix_path.digest.to_vec(), path_info.clone());

                Ok(path_info)
            }
        }
    }
}
