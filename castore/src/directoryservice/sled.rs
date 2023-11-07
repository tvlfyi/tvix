use crate::directoryservice::DirectoryPutter;
use crate::proto::Directory;
use crate::{proto, B3Digest, Error};
use futures::Stream;
use prost::Message;
use std::path::PathBuf;
use std::pin::Pin;
use tonic::async_trait;
use tracing::{instrument, warn};

use super::utils::{traverse_directory, SimplePutter};
use super::DirectoryService;

#[derive(Clone)]
pub struct SledDirectoryService {
    db: sled::Db,
}

impl SledDirectoryService {
    pub fn new(p: PathBuf) -> Result<Self, sled::Error> {
        let config = sled::Config::default().use_compression(true).path(p);
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
impl DirectoryService for SledDirectoryService {
    /// Constructs a [SledDirectoryService] from the passed [url::Url]:
    /// - scheme has to be `sled://`
    /// - there may not be a host.
    /// - a path to the sled needs to be provided (which may not be `/`).
    fn from_url(url: &url::Url) -> Result<Self, Error> {
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
            Self::new_temporary().map_err(|e| Error::StorageError(e.to_string()))
        } else if url.path() == "/" {
            Err(crate::Error::StorageError(
                "cowardly refusing to open / with sled".to_string(),
            ))
        } else {
            Self::new(url.path().into()).map_err(|e| Error::StorageError(e.to_string()))
        }
    }

    #[instrument(skip(self, digest), fields(directory.digest = %digest))]
    async fn get(&self, digest: &B3Digest) -> Result<Option<proto::Directory>, Error> {
        match self.db.get(digest.as_slice()) {
            // The directory was not found, return
            Ok(None) => Ok(None),

            // The directory was found, try to parse the data as Directory message
            Ok(Some(data)) => match Directory::decode(&*data) {
                Ok(directory) => {
                    // Validate the retrieved Directory indeed has the
                    // digest we expect it to have, to detect corruptions.
                    let actual_digest = directory.digest();
                    if actual_digest != *digest {
                        return Err(Error::StorageError(format!(
                            "requested directory with digest {}, but got {}",
                            digest, actual_digest
                        )));
                    }

                    // Validate the Directory itself is valid.
                    if let Err(e) = directory.validate() {
                        warn!("directory failed validation: {}", e.to_string());
                        return Err(Error::StorageError(format!(
                            "directory {} failed validation: {}",
                            actual_digest, e,
                        )));
                    }

                    Ok(Some(directory))
                }
                Err(e) => {
                    warn!("unable to parse directory {}: {}", digest, e);
                    Err(Error::StorageError(e.to_string()))
                }
            },
            // some storage error?
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip(self, directory), fields(directory.digest = %directory.digest()))]
    async fn put(&self, directory: proto::Directory) -> Result<B3Digest, Error> {
        let digest = directory.digest();

        // validate the directory itself.
        if let Err(e) = directory.validate() {
            return Err(Error::InvalidRequest(format!(
                "directory {} failed validation: {}",
                digest, e,
            )));
        }
        // store it
        let result = self.db.insert(digest.as_slice(), directory.encode_to_vec());
        if let Err(e) = result {
            return Err(Error::StorageError(e.to_string()));
        }
        Ok(digest)
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> Pin<Box<(dyn Stream<Item = Result<proto::Directory, Error>> + Send + 'static)>> {
        traverse_directory(self.clone(), root_directory_digest)
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<(dyn DirectoryPutter + 'static)>
    where
        Self: Clone,
    {
        Box::new(SimplePutter::new(self.clone()))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::DirectoryService;
    use super::SledDirectoryService;

    /// This uses a wrong scheme.
    #[test]
    fn test_invalid_scheme() {
        let url = url::Url::parse("http://foo.example/test").expect("must parse");

        assert!(SledDirectoryService::from_url(&url).is_err());
    }

    /// This uses the correct scheme, and doesn't specify a path (temporary sled).
    #[test]
    fn test_valid_scheme_temporary() {
        let url = url::Url::parse("sled://").expect("must parse");

        assert!(SledDirectoryService::from_url(&url).is_ok());
    }

    /// This sets the path to a location that doesn't exist, which should fail (as sled doesn't mkdir -p)
    #[test]
    fn test_nonexistent_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://foo.example").expect("must parse");
        url.set_path(tmpdir.path().join("foo").join("bar").to_str().unwrap());

        assert!(SledDirectoryService::from_url(&url).is_err());
    }

    /// This uses the correct scheme, and specifies / as path (which should fail
    // for obvious reasons)
    #[test]
    fn test_invalid_path_root() {
        let url = url::Url::parse("sled:///").expect("must parse");

        assert!(SledDirectoryService::from_url(&url).is_err());
    }

    /// This uses the correct scheme, and sets a tempdir as location.
    #[test]
    fn test_valid_scheme_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://").expect("must parse");
        url.set_path(tmpdir.path().to_str().unwrap());

        assert!(SledDirectoryService::from_url(&url).is_ok());
    }

    /// This sets a host, rather than a path, which should fail.
    #[test]
    fn test_invalid_host() {
        let url = url::Url::parse("sled://foo.example").expect("must parse");

        assert!(SledDirectoryService::from_url(&url).is_err());
    }

    /// This sets a host AND a valid path, which should fail
    #[test]
    fn test_invalid_host_and_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://foo.example").expect("must parse");
        url.set_path(tmpdir.path().to_str().unwrap());

        assert!(SledDirectoryService::from_url(&url).is_err());
    }
}
