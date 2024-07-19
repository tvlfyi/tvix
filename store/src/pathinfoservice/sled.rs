use super::PathInfoService;
use crate::proto::PathInfo;
use async_stream::try_stream;
use futures::stream::BoxStream;
use nix_compat::nixbase32;
use prost::Message;
use std::path::Path;
use std::sync::Arc;
use tonic::async_trait;
use tracing::{instrument, warn};
use tvix_castore::composition::{CompositionContext, ServiceBuilder};
use tvix_castore::Error;

/// SledPathInfoService stores PathInfo in a [sled](https://github.com/spacejam/sled).
///
/// The PathInfo messages are stored as encoded protos, and keyed by their output hash,
/// as that's currently the only request type available.
pub struct SledPathInfoService {
    db: sled::Db,
}

impl SledPathInfoService {
    pub fn new<P: AsRef<Path>>(p: P) -> Result<Self, sled::Error> {
        let config = sled::Config::default()
            .use_compression(false) // is a required parameter
            .path(p);
        let db = config.open()?;

        Ok(Self { db })
    }

    pub fn new_temporary() -> Result<Self, sled::Error> {
        let config = sled::Config::default().temporary(true);
        let db = config.open()?;

        Ok(Self { db })
    }
}

#[async_trait]
impl PathInfoService for SledPathInfoService {
    #[instrument(level = "trace", skip_all, fields(path_info.digest = nixbase32::encode(&digest)))]
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        let resp = tokio::task::spawn_blocking({
            let db = self.db.clone();
            move || db.get(digest.as_slice())
        })
        .await?
        .map_err(|e| {
            warn!("failed to retrieve PathInfo: {}", e);
            Error::StorageError(format!("failed to retrieve PathInfo: {}", e))
        })?;
        match resp {
            None => Ok(None),
            Some(data) => {
                let path_info = PathInfo::decode(&*data).map_err(|e| {
                    warn!("failed to decode stored PathInfo: {}", e);
                    Error::StorageError(format!("failed to decode stored PathInfo: {}", e))
                })?;
                Ok(Some(path_info))
            }
        }
    }

    #[instrument(level = "trace", skip_all, fields(path_info.root_node = ?path_info.node))]
    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error> {
        // Call validate on the received PathInfo message.
        let store_path = path_info
            .validate()
            .map_err(|e| Error::InvalidRequest(format!("failed to validate PathInfo: {}", e)))?;

        // In case the PathInfo is valid, we were able to parse a StorePath.
        // Store it in the database, keyed by its digest.
        // This overwrites existing PathInfo objects.
        tokio::task::spawn_blocking({
            let db = self.db.clone();
            let k = *store_path.digest();
            let data = path_info.encode_to_vec();
            move || db.insert(k, data)
        })
        .await?
        .map_err(|e| {
            warn!("failed to insert PathInfo: {}", e);
            Error::StorageError(format! {
                "failed to insert PathInfo: {}", e
            })
        })?;

        Ok(path_info)
    }

    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
        let db = self.db.clone();
        let mut it = db.iter().values();

        Box::pin(try_stream! {
            // Don't block the executor while waiting for .next(), so wrap that
            // in a spawn_blocking call.
            // We need to pass around it to be able to reuse it.
            while let (Some(elem), new_it) = tokio::task::spawn_blocking(move || {
                (it.next(), it)
            }).await? {
                it = new_it;
                let data = elem.map_err(|e| {
                    warn!("failed to retrieve PathInfo: {}", e);
                    Error::StorageError(format!("failed to retrieve PathInfo: {}", e))
                })?;

                let path_info = PathInfo::decode(&*data).map_err(|e| {
                    warn!("failed to decode stored PathInfo: {}", e);
                    Error::StorageError(format!("failed to decode stored PathInfo: {}", e))
                })?;

                yield path_info
            }
        })
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SledPathInfoServiceConfig {
    is_temporary: bool,
    #[serde(default)]
    /// required when is_temporary = false
    path: Option<String>,
}

impl TryFrom<url::Url> for SledPathInfoServiceConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        // sled doesn't support host, and a path can be provided (otherwise
        // it'll live in memory only).
        if url.has_host() {
            return Err(Error::StorageError("no host allowed".to_string()).into());
        }

        // TODO: expose compression and other parameters as URL parameters?

        Ok(if url.path().is_empty() {
            SledPathInfoServiceConfig {
                is_temporary: true,
                path: None,
            }
        } else {
            SledPathInfoServiceConfig {
                is_temporary: false,
                path: Some(url.path().to_string()),
            }
        })
    }
}

#[async_trait]
impl ServiceBuilder for SledPathInfoServiceConfig {
    type Output = dyn PathInfoService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        _context: &CompositionContext,
    ) -> Result<Arc<dyn PathInfoService>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        match self {
            SledPathInfoServiceConfig {
                is_temporary: true,
                path: None,
            } => Ok(Arc::new(SledPathInfoService::new_temporary()?)),
            SledPathInfoServiceConfig {
                is_temporary: true,
                path: Some(_),
            } => Err(
                Error::StorageError("Temporary SledPathInfoService can not have path".into())
                    .into(),
            ),
            SledPathInfoServiceConfig {
                is_temporary: false,
                path: None,
            } => Err(Error::StorageError("SledPathInfoService is missing path".into()).into()),
            SledPathInfoServiceConfig {
                is_temporary: false,
                path: Some(path),
            } => Ok(Arc::new(SledPathInfoService::new(path)?)),
        }
    }
}
