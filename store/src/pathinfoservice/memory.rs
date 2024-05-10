use super::PathInfoService;
use crate::proto::PathInfo;
use futures::stream::{iter, BoxStream};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tonic::async_trait;
use tvix_castore::Error;

#[derive(Default)]
pub struct MemoryPathInfoService {
    db: Arc<RwLock<HashMap<[u8; 20], PathInfo>>>,
}

#[async_trait]
impl PathInfoService for MemoryPathInfoService {
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        let db = self.db.read().unwrap();

        match db.get(&digest) {
            None => Ok(None),
            Some(path_info) => Ok(Some(path_info.clone())),
        }
    }

    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error> {
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
                db.insert(*nix_path.digest(), path_info.clone());

                Ok(path_info)
            }
        }
    }

    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
        let db = self.db.read().unwrap();

        // Copy all elements into a list.
        // This is a bit ugly, because we can't have db escape the lifetime
        // of this function, but elements need to be returned owned anyways, and this in-
        // memory impl is only for testing purposes anyways.
        let items: Vec<_> = db.iter().map(|(_k, v)| Ok(v.clone())).collect();

        Box::pin(iter(items))
    }
}
