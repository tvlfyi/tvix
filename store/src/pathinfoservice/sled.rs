use super::PathInfoService;
use crate::nar::calculate_size_and_sha256;
use crate::proto::PathInfo;
use async_stream::try_stream;
use data_encoding::BASE64;
use futures::stream::BoxStream;
use prost::Message;
use std::path::Path;
use tonic::async_trait;
use tracing::instrument;
use tracing::warn;
use tvix_castore::proto as castorepb;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService, Error};

/// SledPathInfoService stores PathInfo in a [sled](https://github.com/spacejam/sled).
///
/// The PathInfo messages are stored as encoded protos, and keyed by their output hash,
/// as that's currently the only request type available.
pub struct SledPathInfoService<BS, DS> {
    db: sled::Db,

    blob_service: BS,
    directory_service: DS,
}

impl<BS, DS> SledPathInfoService<BS, DS> {
    pub fn new<P: AsRef<Path>>(
        p: P,
        blob_service: BS,
        directory_service: DS,
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

    pub fn new_temporary(blob_service: BS, directory_service: DS) -> Result<Self, sled::Error> {
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
impl<BS, DS> PathInfoService for SledPathInfoService<BS, DS>
where
    BS: AsRef<dyn BlobService> + Send + Sync,
    DS: AsRef<dyn DirectoryService> + Send + Sync,
{
    #[instrument(level = "trace", skip_all, fields(path_info.digest = BASE64.encode(&digest)))]
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

    #[instrument(level = "trace", skip_all, fields(root_node = ?root_node))]
    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), Error> {
        calculate_size_and_sha256(root_node, &self.blob_service, &self.directory_service)
            .await
            .map_err(|e| Error::StorageError(e.to_string()))
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
