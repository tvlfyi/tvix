use crate::proto::Directory;
use crate::{proto, Error};
use data_encoding::BASE64;
use prost::Message;
use std::path::PathBuf;
use tracing::{instrument, warn};

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

impl DirectoryService for SledDirectoryService {
    // TODO: change api to only be by digest
    #[instrument(name = "SledDirectoryService::get", skip(self, by_what))]
    fn get(
        &self,
        by_what: &proto::get_directory_request::ByWhat,
    ) -> Result<Option<proto::Directory>, Error> {
        match by_what {
            proto::get_directory_request::ByWhat::Digest(digest) => {
                match self.db.get(digest) {
                    // The directory was not found, return
                    Ok(None) => Ok(None),

                    // The directory was found, try to parse the data as Directory message
                    Ok(Some(data)) => match Directory::decode(&*data) {
                        Ok(directory) => {
                            // Validate the retrieved Directory indeed has the
                            // digest we expect it to have, to detect corruptions.
                            let actual_digest = directory.digest();
                            if actual_digest.as_slice() != digest {
                                return Err(Error::StorageError(format!(
                                    "requested directory with digest {}, but got {}",
                                    BASE64.encode(digest),
                                    BASE64.encode(&actual_digest)
                                )));
                            }

                            Ok(Some(directory))
                        }
                        Err(e) => {
                            warn!("unable to parse directory {}: {}", BASE64.encode(digest), e);
                            Err(Error::StorageError(e.to_string()))
                        }
                    },
                    // some storage error?
                    Err(e) => Err(Error::StorageError(e.to_string())),
                }
            }
        }
    }

    #[instrument(name = "SledDirectoryService::put", skip(self, directory), fields(directory.digest = BASE64.encode(&directory.digest())))]
    fn put(&self, directory: proto::Directory) -> Result<Vec<u8>, Error> {
        let digest = directory.digest();

        // validate the directory itself.
        if let Err(e) = directory.validate() {
            return Err(Error::InvalidRequest(format!(
                "directory {} failed validation: {}",
                BASE64.encode(&digest),
                e,
            )));
        }
        // store it
        let result = self.db.insert(&digest, directory.encode_to_vec());
        if let Err(e) = result {
            return Err(Error::StorageError(e.to_string()));
        }
        Ok(digest)
    }
}
