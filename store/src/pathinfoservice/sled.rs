use super::PathInfoService;
use crate::{
    blobservice::BlobService, directoryservice::DirectoryService, nar::calculate_size_and_sha256,
    proto, Error,
};
use futures::{stream::iter, Stream};
use prost::Message;
use std::{path::PathBuf, pin::Pin, sync::Arc};
use tonic::async_trait;
use tracing::warn;

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
        let config = sled::Config::default().use_compression(true).path(p);
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
    /// Constructs a [SledPathInfoService] from the passed [url::Url]:
    /// - scheme has to be `sled://`
    /// - there may not be a host.
    /// - a path to the sled needs to be provided (which may not be `/`).
    fn from_url(
        url: &url::Url,
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) -> Result<Self, Error> {
        if url.scheme() != "sled" {
            return Err(crate::Error::StorageError("invalid scheme".to_string()));
        }

        if url.has_host() {
            return Err(crate::Error::StorageError(format!(
                "invalid host: {}",
                url.host().unwrap()
            )));
        }

        // TODO: expose compression and other parameters as URL parameters, drop new and new_temporary?
        if url.path().is_empty() {
            Self::new_temporary(blob_service, directory_service)
                .map_err(|e| Error::StorageError(e.to_string()))
        } else if url.path() == "/" {
            Err(crate::Error::StorageError(
                "cowardly refusing to open / with sled".to_string(),
            ))
        } else {
            Self::new(url.path().into(), blob_service, directory_service)
                .map_err(|e| Error::StorageError(e.to_string()))
        }
    }

    async fn get(&self, digest: [u8; 20]) -> Result<Option<proto::PathInfo>, Error> {
        match self.db.get(digest) {
            Ok(None) => Ok(None),
            Ok(Some(data)) => match proto::PathInfo::decode(&*data) {
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

    async fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, Error> {
        // Call validate on the received PathInfo message.
        match path_info.validate() {
            Err(e) => Err(Error::InvalidRequest(format!(
                "failed to validate PathInfo: {}",
                e
            ))),
            // In case the PathInfo is valid, and we were able to extract a NixPath, store it in the database.
            // This overwrites existing PathInfo objects.
            Ok(nix_path) => match self.db.insert(nix_path.digest, path_info.encode_to_vec()) {
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

    async fn calculate_nar(&self, root_node: &proto::node::Node) -> Result<(u64, [u8; 32]), Error> {
        calculate_size_and_sha256(
            root_node,
            self.blob_service.clone(),
            self.directory_service.clone(),
        )
        .await
        .map_err(|e| Error::StorageError(e.to_string()))
    }

    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<proto::PathInfo, Error>> + Send>> {
        Box::pin(iter(self.db.iter().values().map(|v| match v {
            Ok(data) => {
                // we retrieved some bytes
                match proto::PathInfo::decode(&*data) {
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::tests::utils::gen_blob_service;
    use crate::tests::utils::gen_directory_service;

    use super::PathInfoService;
    use super::SledPathInfoService;

    /// This uses a wrong scheme.
    #[test]
    fn test_invalid_scheme() {
        let url = url::Url::parse("http://foo.example/test").expect("must parse");

        assert!(
            SledPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }

    /// This uses the correct scheme, and doesn't specify a path (temporary sled).
    #[test]
    fn test_valid_scheme_temporary() {
        let url = url::Url::parse("sled://").expect("must parse");

        assert!(
            SledPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_ok()
        );
    }

    /// This sets the path to a location that doesn't exist, which should fail (as sled doesn't mkdir -p)
    #[test]
    fn test_nonexistent_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://foo.example").expect("must parse");
        url.set_path(tmpdir.path().join("foo").join("bar").to_str().unwrap());

        assert!(
            SledPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }

    /// This uses the correct scheme, and specifies / as path (which should fail
    // for obvious reasons)
    #[test]
    fn test_invalid_path_root() {
        let url = url::Url::parse("sled:///").expect("must parse");

        assert!(
            SledPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }

    /// This uses the correct scheme, and sets a tempdir as location.
    #[test]
    fn test_valid_scheme_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://").expect("must parse");
        url.set_path(tmpdir.path().to_str().unwrap());

        assert!(
            SledPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_ok()
        );
    }

    /// This sets a host, rather than a path, which should fail.
    #[test]
    fn test_invalid_host() {
        let url = url::Url::parse("sled://foo.example").expect("must parse");

        assert!(
            SledPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }

    /// This sets a host AND a valid path, which should fail
    #[test]
    fn test_invalid_host_and_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://foo.example").expect("must parse");
        url.set_path(tmpdir.path().to_str().unwrap());

        assert!(
            SledPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }
}
