use super::PathInfoService;
use crate::{nar::calculate_size_and_sha256, proto::PathInfo};
use futures::{stream::iter, Stream};
use std::{
    collections::HashMap,
    pin::Pin,
    sync::{Arc, RwLock},
};
use tonic::async_trait;
use tvix_castore::proto as castorepb;
use tvix_castore::Error;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService};

pub struct MemoryPathInfoService<BS, DS> {
    db: Arc<RwLock<HashMap<[u8; 20], PathInfo>>>,

    blob_service: BS,
    directory_service: DS,
}

impl<BS, DS> MemoryPathInfoService<BS, DS> {
    pub fn new(blob_service: BS, directory_service: DS) -> Self {
        Self {
            db: Default::default(),
            blob_service,
            directory_service,
        }
    }
}

#[async_trait]
impl<BS, DS> PathInfoService for MemoryPathInfoService<BS, DS>
where
    BS: AsRef<dyn BlobService> + Send + Sync,
    DS: AsRef<dyn DirectoryService> + Send + Sync,
{
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

    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), Error> {
        calculate_size_and_sha256(root_node, &self.blob_service, &self.directory_service)
            .await
            .map_err(|e| Error::StorageError(e.to_string()))
    }

    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<PathInfo, Error>> + Send>> {
        let db = self.db.read().unwrap();

        // Copy all elements into a list.
        // This is a bit ugly, because we can't have db escape the lifetime
        // of this function, but elements need to be returned owned anyways, and this in-
        // memory impl is only for testing purposes anyways.
        let items: Vec<_> = db.iter().map(|(_k, v)| Ok(v.clone())).collect();

        Box::pin(iter(items))
    }
}
