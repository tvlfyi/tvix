use crate::nar::RenderError;
use crate::pathinfoservice::PathInfoService;
use crate::proto;
use std::sync::Arc;
use tonic::{async_trait, Request, Response, Result, Status};
use tracing::{instrument, warn};

pub struct GRPCPathInfoServiceWrapper {
    path_info_service: Arc<dyn PathInfoService>,
}

impl From<Arc<dyn PathInfoService>> for GRPCPathInfoServiceWrapper {
    fn from(value: Arc<dyn PathInfoService>) -> Self {
        Self {
            path_info_service: value,
        }
    }
}

#[async_trait]
impl proto::path_info_service_server::PathInfoService for GRPCPathInfoServiceWrapper {
    #[instrument(skip(self))]
    async fn get(
        &self,
        request: Request<proto::GetPathInfoRequest>,
    ) -> Result<Response<proto::PathInfo>> {
        match request.into_inner().by_what {
            None => Err(Status::unimplemented("by_what needs to be specified")),
            Some(proto::get_path_info_request::ByWhat::ByOutputHash(output_digest)) => {
                let digest: [u8; 20] = output_digest
                    .to_vec()
                    .try_into()
                    .map_err(|_e| Status::invalid_argument("invalid output digest length"))?;
                match self.path_info_service.get(digest) {
                    Ok(None) => Err(Status::not_found("PathInfo not found")),
                    Ok(Some(path_info)) => Ok(Response::new(path_info)),
                    Err(e) => {
                        warn!("failed to retrieve PathInfo: {}", e);
                        Err(e.into())
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    async fn put(&self, request: Request<proto::PathInfo>) -> Result<Response<proto::PathInfo>> {
        let path_info = request.into_inner();

        // Store the PathInfo in the client. Clients MUST validate the data
        // they receive, so we don't validate additionally here.
        match self.path_info_service.put(path_info) {
            Ok(path_info_new) => Ok(Response::new(path_info_new)),
            Err(e) => {
                warn!("failed to insert PathInfo: {}", e);
                Err(e.into())
            }
        }
    }

    #[instrument(skip(self))]
    async fn calculate_nar(
        &self,
        request: Request<proto::Node>,
    ) -> Result<Response<proto::CalculateNarResponse>> {
        match request.into_inner().node {
            None => Err(Status::invalid_argument("no root node sent")),
            Some(root_node) => {
                let (nar_size, nar_sha256) = self
                    .path_info_service
                    .calculate_nar(&root_node)
                    .expect("error during nar calculation"); // TODO: handle error

                Ok(Response::new(proto::CalculateNarResponse {
                    nar_size,
                    nar_sha256: nar_sha256.to_vec().into(),
                }))
            }
        }
    }
}

impl From<RenderError> for tonic::Status {
    fn from(value: RenderError) -> Self {
        match value {
            RenderError::BlobNotFound(_, _) => Self::not_found(value.to_string()),
            RenderError::DirectoryNotFound(_, _) => Self::not_found(value.to_string()),
            RenderError::NARWriterError(_) => Self::internal(value.to_string()),
            RenderError::StoreError(_) => Self::internal(value.to_string()),
            RenderError::UnexpectedBlobMeta(_, _, _, _) => Self::internal(value.to_string()),
        }
    }
}
