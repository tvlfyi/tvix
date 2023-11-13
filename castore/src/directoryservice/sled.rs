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
