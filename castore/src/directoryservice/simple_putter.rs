use super::DirectoryPutter;
use super::DirectoryService;
use super::{DirectoryGraph, LeavesToRootValidator};
use crate::proto;
use crate::B3Digest;
use crate::Error;
use tonic::async_trait;
use tracing::instrument;
use tracing::warn;

/// This is an implementation of DirectoryPutter that simply
/// inserts individual Directory messages one by one, on close, after
/// they successfully validated.
pub struct SimplePutter<DS: DirectoryService> {
    directory_service: DS,

    directory_validator: Option<DirectoryGraph<LeavesToRootValidator>>,
}

impl<DS: DirectoryService> SimplePutter<DS> {
    pub fn new(directory_service: DS) -> Self {
        Self {
            directory_service,
            directory_validator: Some(Default::default()),
        }
    }
}

#[async_trait]
impl<DS: DirectoryService + 'static> DirectoryPutter for SimplePutter<DS> {
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

                // call an individual put for each directory and await the insertion.
                for directory in directories {
                    let exp_digest = directory.digest();
                    let actual_digest = self.directory_service.put(directory).await?;

                    // ensure the digest the backend told us matches our expectations.
                    if exp_digest != actual_digest {
                        warn!(directory.digest_expected=%exp_digest, directory.digest_actual=%actual_digest, "unexpected digest");
                        return Err(Error::StorageError(
                            "got unexpected digest from backend during put".into(),
                        ));
                    }
                }

                Ok(root_digest)
            }
        }
    }
}
