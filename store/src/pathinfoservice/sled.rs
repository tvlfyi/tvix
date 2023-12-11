use super::PathInfoService;
use crate::nar::calculate_size_and_sha256;
use crate::proto::PathInfo;
use futures::{stream::iter, Stream};
use prost::Message;
use std::{path::PathBuf, pin::Pin, sync::Arc};
use tonic::async_trait;
use tracing::warn;
use tvix_castore::proto as castorepb;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService, Error};

/// SledPathInfoService stores PathInfo in a [sled](https://github.com/spacejam/sled).
///
/// The PathInfo messages are stored as encoded protos, and keyed by their output hash,
/// as that's currently the only request type available.
pub struct SledPathInfoService {
    db: sled::Db,

    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
}

impl SledPathInfoService {
    pub fn new(
        p: PathBuf,
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) -> Result<Self, sled::Error> {
        let config = sled::Config::default()
            .use_compression(false) // is a required parameter
            .path(p);
        let db = config.open()?;

        Ok(Self {
            db,
            blob_service,
            directory_service,
        })
    }

    pub fn new_temporary(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) -> Result<Self, sled::Error> {
        let config = sled::Config::default().temporary(true);
        let db = config.open()?;

        Ok(Self {
            db,
            blob_service,
            directory_service,
        })
    }
}

#[async_trait]
impl PathInfoService for SledPathInfoService {
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        match self.db.get(digest) {
            Ok(None) => Ok(None),
            Ok(Some(data)) => match PathInfo::decode(&*data) {
                Ok(path_info) => Ok(Some(path_info)),
                Err(e) => {
                    warn!("failed to decode stored PathInfo: {}", e);
                    Err(Error::StorageError(format!(
                        "failed to decode stored PathInfo: {}",
                        e
                    )))
                }
            },
            Err(e) => {
                warn!("failed to retrieve PathInfo: {}", e);
                Err(Error::StorageError(format!(
                    "failed to retrieve PathInfo: {}",
                    e
                )))
            }
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
            Ok(nix_path) => match self
                .db
                .insert(*nix_path.digest(), path_info.encode_to_vec())
            {
                Ok(_) => Ok(path_info),
                Err(e) => {
                    warn!("failed to insert PathInfo: {}", e);
                    Err(Error::StorageError(format! {
                        "failed to insert PathInfo: {}", e
                    }))
                }
            },
        }
    }

    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), Error> {
        calculate_size_and_sha256(
            root_node,
            self.blob_service.clone(),
            self.directory_service.clone(),
        )
        .await
        .map_err(|e| Error::StorageError(e.to_string()))
    }

    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<PathInfo, Error>> + Send>> {
        Box::pin(iter(self.db.iter().values().map(|v| match v {
            Ok(data) => {
                // we retrieved some bytes
                match PathInfo::decode(&*data) {
                    Ok(path_info) => Ok(path_info),
                    Err(e) => {
                        warn!("failed to decode stored PathInfo: {}", e);
                        Err(Error::StorageError(format!(
                            "failed to decode stored PathInfo: {}",
                            e
                        )))
                    }
                }
            }
            Err(e) => {
                warn!("failed to retrieve PathInfo: {}", e);
                Err(Error::StorageError(format!(
                    "failed to retrieve PathInfo: {}",
                    e
                )))
            }
        })))
    }
}
