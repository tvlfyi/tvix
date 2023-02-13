use crate::{proto, Error};
use nix_compat::store_path::DIGEST_SIZE;
use prost::Message;
use std::path::PathBuf;
use tracing::warn;

use super::PathInfoService;

/// SledPathInfoService stores PathInfo in a [sled](https://github.com/spacejam/sled).
///
/// The PathInfo messages are stored as encoded protos, and keyed by their output hash,
/// as that's currently the only request type available.
pub struct SledPathInfoService {
    db: sled::Db,
}

impl SledPathInfoService {
    pub fn new(p: PathBuf) -> Result<Self, sled::Error> {
        let config = sled::Config::default().use_compression(true).path(p);
        let db = config.open()?;

        Ok(Self { db })
    }
}

impl PathInfoService for SledPathInfoService {
    fn get(
        &self,
        by_what: proto::get_path_info_request::ByWhat,
    ) -> Result<Option<proto::PathInfo>, Error> {
        match by_what {
            proto::get_path_info_request::ByWhat::ByOutputHash(digest) => {
                if digest.len() != DIGEST_SIZE {
                    return Err(Error::InvalidRequest("invalid digest length".to_string()));
                }

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
        }
    }

    fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, Error> {
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
}
