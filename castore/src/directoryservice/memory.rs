use crate::{proto, B3Digest, Error};
use futures::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use tonic::async_trait;
use tracing::{instrument, warn};

use super::utils::{traverse_directory, SimplePutter};
use super::{DirectoryPutter, DirectoryService};

#[derive(Clone, Default)]
pub struct MemoryDirectoryService {
    db: Arc<RwLock<HashMap<B3Digest, proto::Directory>>>,
}

#[async_trait]
impl DirectoryService for MemoryDirectoryService {
    #[instrument(skip(self, digest), fields(directory.digest = %digest))]
    async fn get(&self, digest: &B3Digest) -> Result<Option<proto::Directory>, Error> {
        let db = self.db.read()?;

        match db.get(digest) {
            // The directory was not found, return
            None => Ok(None),

            // The directory was found, try to parse the data as Directory message
            Some(directory) => {
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

                Ok(Some(directory.clone()))
            }
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
        let mut db = self.db.write()?;
        db.insert(digest.clone(), directory);

        Ok(digest)
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> Pin<Box<dyn Stream<Item = Result<proto::Directory, Error>> + Send>> {
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
