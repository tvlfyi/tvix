use super::PathInfoService;
use crate::proto::PathInfo;
use async_stream::try_stream;
use futures::stream::BoxStream;
use nix_compat::nixbase32;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tonic::async_trait;
use tracing::instrument;
use tvix_castore::Error;

#[derive(Default)]
pub struct MemoryPathInfoService {
    db: Arc<RwLock<HashMap<[u8; 20], PathInfo>>>,
}

#[async_trait]
impl PathInfoService for MemoryPathInfoService {
    #[instrument(level = "trace", skip_all, fields(path_info.digest = nixbase32::encode(&digest)))]
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        let db = self.db.read().await;

        match db.get(&digest) {
            None => Ok(None),
            Some(path_info) => Ok(Some(path_info.clone())),
        }
    }

    #[instrument(level = "trace", skip_all, fields(path_info.root_node = ?path_info.node))]
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
                let mut db = self.db.write().await;
                db.insert(*nix_path.digest(), path_info.clone());

                Ok(path_info)
            }
        }
    }

    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
        let db = self.db.clone();

        Box::pin(try_stream! {
            let db = db.read().await;
            let it = db.iter();

            for (_k, v) in it {
                yield v.clone()
            }
        })
    }
}
