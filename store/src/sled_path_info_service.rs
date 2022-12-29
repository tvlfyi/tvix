use prost::Message;
use std::path::PathBuf;

use crate::proto::get_path_info_request::ByWhat;
use crate::proto::path_info_service_server::PathInfoService;
use crate::proto::CalculateNarResponse;
use crate::proto::GetPathInfoRequest;
use crate::proto::Node;
use crate::proto::PathInfo;
use crate::store_path::DIGEST_SIZE;
use tonic::{Request, Response, Result, Status};
use tracing::{instrument, warn};

const NOT_IMPLEMENTED_MSG: &str = "not implemented";

/// SledPathInfoService stores PathInfo in a [sled](https://github.com/spacejam/sled).
///
/// The PathInfo messages are stored as encoded protos, and keyed by their output hash,
/// as that's currently the only request type available.
pub struct SledPathInfoService {
    db: sled::Db,
}

impl SledPathInfoService {
    pub fn new(p: PathBuf) -> Result<Self, anyhow::Error> {
        let config = sled::Config::default().use_compression(true).path(p);
        let db = config.open()?;

        Ok(Self { db })
    }
}

#[tonic::async_trait]
impl PathInfoService for SledPathInfoService {
    #[instrument(skip(self))]
    async fn get(&self, request: Request<GetPathInfoRequest>) -> Result<Response<PathInfo>> {
        match request.into_inner().by_what {
            None => Err(Status::unimplemented("by_what needs to be specified")),
            Some(ByWhat::ByOutputHash(digest)) => {
                if digest.len() != DIGEST_SIZE {
                    return Err(Status::invalid_argument("invalid digest length"));
                }

                match self.db.get(digest) {
                    Ok(None) => Err(Status::not_found("PathInfo not found")),
                    Ok(Some(data)) => match PathInfo::decode(&*data) {
                        Ok(path_info) => Ok(Response::new(path_info)),
                        Err(e) => {
                            warn!("failed to decode stored PathInfo: {}", e);
                            Err(Status::internal("failed to decode stored PathInfo"))
                        }
                    },
                    Err(e) => {
                        warn!("failed to retrieve PathInfo: {}", e);
                        Err(Status::internal("error during PathInfo lookup"))
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    async fn put(&self, request: Request<PathInfo>) -> Result<Response<PathInfo>> {
        let path_info = request.into_inner();

        // Call validate on the received PathInfo message.
        match path_info.validate() {
            Err(e) => Err(Status::invalid_argument(e.to_string())),
            // In case the PathInfo is valid, and we were able to extract a NixPath, store it in the database.
            // This overwrites existing PathInfo objects.
            Ok(nix_path) => match self.db.insert(nix_path.digest, path_info.encode_to_vec()) {
                Ok(_) => Ok(Response::new(path_info)),
                Err(e) => {
                    warn!("failed to insert PathInfo: {}", e);
                    Err(Status::internal("failed to insert PathInfo"))
                }
            },
        }
    }

    #[instrument(skip(self))]
    async fn calculate_nar(
        &self,
        _request: Request<Node>,
    ) -> Result<Response<CalculateNarResponse>> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
    }
}
