use super::{PathInfo, PathInfoService};
use async_stream::try_stream;
use futures::stream::BoxStream;
use nix_compat::nixbase32;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tonic::async_trait;
use tracing::instrument;
use tvix_castore::composition::{CompositionContext, ServiceBuilder};
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
        // This overwrites existing PathInfo objects with the same store path digest.
        let mut db = self.db.write().await;
        db.insert(*path_info.store_path.digest(), path_info.clone());

        Ok(path_info)
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

#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct MemoryPathInfoServiceConfig {}

impl TryFrom<url::Url> for MemoryPathInfoServiceConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        // memory doesn't support host or path in the URL.
        if url.has_host() || !url.path().is_empty() {
            return Err(Error::StorageError("invalid url".to_string()).into());
        }
        Ok(MemoryPathInfoServiceConfig {})
    }
}

#[async_trait]
impl ServiceBuilder for MemoryPathInfoServiceConfig {
    type Output = dyn PathInfoService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        _context: &CompositionContext,
    ) -> Result<Arc<dyn PathInfoService>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        Ok(Arc::new(MemoryPathInfoService::default()))
    }
}
