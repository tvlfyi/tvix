use crate::proto::Directory;
use crate::{proto, B3Digest, Error};
use futures::stream::BoxStream;
use prost::Message;
use std::ops::Deref;
use std::path::Path;
use tonic::async_trait;
use tracing::{instrument, warn};

use super::utils::traverse_directory;
use super::{DirectoryGraph, DirectoryPutter, DirectoryService, LeavesToRootValidator};

#[derive(Clone)]
pub struct SledDirectoryService {
    db: sled::Db,
}

impl SledDirectoryService {
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
impl DirectoryService for SledDirectoryService {
    #[instrument(skip(self, digest), fields(directory.digest = %digest))]
    async fn get(&self, digest: &B3Digest) -> Result<Option<proto::Directory>, Error> {
        let resp = tokio::task::spawn_blocking({
            let db = self.db.clone();
            let digest = digest.clone();
            move || db.get(digest.as_slice())
        })
        .await?
        .map_err(|e| {
            warn!("failed to retrieve directory: {}", e);
            Error::StorageError(format!("failed to retrieve directory: {}", e))
        })?;

        match resp {
            // The directory was not found, return
            None => Ok(None),

            // The directory was found, try to parse the data as Directory message
            Some(data) => match Directory::decode(&*data) {
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
        }
    }

    #[instrument(skip(self, directory), fields(directory.digest = %directory.digest()))]
    async fn put(&self, directory: proto::Directory) -> Result<B3Digest, Error> {
        tokio::task::spawn_blocking({
            let db = self.db.clone();
            move || {
                let digest = directory.digest();

                // validate the directory itself.
                if let Err(e) = directory.validate() {
                    return Err(Error::InvalidRequest(format!(
                        "directory {} failed validation: {}",
                        digest, e,
                    )));
                }
                // store it
                db.insert(digest.as_slice(), directory.encode_to_vec())
                    .map_err(|e| Error::StorageError(e.to_string()))?;

                Ok(digest)
            }
        })
        .await?
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> BoxStream<'static, Result<proto::Directory, Error>> {
        traverse_directory(self.clone(), root_directory_digest)
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<(dyn DirectoryPutter + 'static)>
    where
        Self: Clone,
    {
        Box::new(SledDirectoryPutter {
            tree: self.db.deref().clone(),
            directory_validator: Some(Default::default()),
        })
    }
}

/// Buffers Directory messages to be uploaded and inserts them in a batch
/// transaction on close.
pub struct SledDirectoryPutter {
    tree: sled::Tree,

    /// The directories (inside the directory validator) that we insert later,
    /// or None, if they were already inserted.
    directory_validator: Option<DirectoryGraph<LeavesToRootValidator>>,
}

#[async_trait]
impl DirectoryPutter for SledDirectoryPutter {
    #[instrument(level = "trace", skip_all, fields(directory.digest=%directory.digest()), err)]
    async fn put(&mut self, directory: proto::Directory) -> Result<(), Error> {
        match self.directory_validator {
            None => return Err(Error::StorageError("already closed".to_string())),
            Some(ref mut validator) => {
                validator
                    .add(directory)
                    .map_err(|e| Error::StorageError(e.to_string()))?;
            }
        }

        Ok(())
    }

    #[instrument(level = "trace", skip_all, ret, err)]
    async fn close(&mut self) -> Result<B3Digest, Error> {
        match self.directory_validator.take() {
            None => Err(Error::InvalidRequest("already closed".to_string())),
            Some(validator) => {
                // Insert all directories as a batch.
                tokio::task::spawn_blocking({
                    let tree = self.tree.clone();
                    move || {
                        // retrieve the validated directories.
                        let directories = validator
                            .validate()
                            .map_err(|e| Error::StorageError(e.to_string()))?
                            .drain_leaves_to_root()
                            .collect::<Vec<_>>();

                        // Get the root digest, which is at the end (cf. insertion order)
                        let root_digest = directories
                            .last()
                            .ok_or_else(|| Error::InvalidRequest("got no directories".to_string()))?
                            .digest();

                        let mut batch = sled::Batch::default();
                        for directory in directories {
                            batch.insert(directory.digest().as_slice(), directory.encode_to_vec());
                        }

                        tree.apply_batch(batch).map_err(|e| {
                            Error::StorageError(format!("unable to apply batch: {}", e))
                        })?;

                        Ok(root_digest)
                    }
                })
                .await?
            }
        }
    }
}
