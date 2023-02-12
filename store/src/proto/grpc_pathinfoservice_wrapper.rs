use crate::nar::RenderError;
use crate::proto;
use crate::{nar::NARCalculationService, pathinfoservice::PathInfoService};
use tonic::{async_trait, Request, Response, Result, Status};
use tracing::{instrument, warn};

pub struct GRPCPathInfoServiceWrapper<PS: PathInfoService, NS: NARCalculationService> {
    path_info_service: PS,
    nar_calculation_service: NS,
}

impl<PS: PathInfoService, NS: NARCalculationService> GRPCPathInfoServiceWrapper<PS, NS> {
    pub fn new(path_info_service: PS, nar_calculation_service: NS) -> Self {
        Self {
            path_info_service,
            nar_calculation_service,
        }
    }
}

#[async_trait]
impl<
        PS: PathInfoService + Send + Sync + 'static,
        NS: NARCalculationService + Send + Sync + 'static,
    > proto::path_info_service_server::PathInfoService for GRPCPathInfoServiceWrapper<PS, NS>
{
    #[instrument(skip(self))]
    async fn get(
        &self,
        request: Request<proto::GetPathInfoRequest>,
    ) -> Result<Response<proto::PathInfo>> {
        match request.into_inner().by_what {
            None => Err(Status::unimplemented("by_what needs to be specified")),
            Some(by_what) => match self.path_info_service.get(by_what) {
                Ok(None) => Err(Status::not_found("PathInfo not found")),
                Ok(Some(path_info)) => Ok(Response::new(path_info)),
                Err(e) => {
                    warn!("failed to retrieve PathInfo: {}", e);
                    Err(e.into())
                }
            },
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
            Some(root_node) => match self.nar_calculation_service.calculate_nar(root_node) {
                Ok(resp) => Ok(Response::new(resp)),
                Err(e) => Err(e.into()),
            },
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
