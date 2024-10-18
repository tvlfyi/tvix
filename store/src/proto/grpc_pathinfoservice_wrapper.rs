use crate::nar::{NarCalculationService, RenderError};
use crate::pathinfoservice::{PathInfo, PathInfoService};
use crate::proto;
use futures::{stream::BoxStream, TryStreamExt};
use std::ops::Deref;
use tonic::{async_trait, Request, Response, Result, Status};
use tracing::{instrument, warn};
use tvix_castore::proto as castorepb;

pub struct GRPCPathInfoServiceWrapper<PS, NS> {
    path_info_service: PS,
    // FUTUREWORK: allow exposing without allowing listing
    nar_calculation_service: NS,
}

impl<PS, NS> GRPCPathInfoServiceWrapper<PS, NS> {
    pub fn new(path_info_service: PS, nar_calculation_service: NS) -> Self {
        Self {
            path_info_service,
            nar_calculation_service,
        }
    }
}

#[async_trait]
impl<PS, NS> proto::path_info_service_server::PathInfoService for GRPCPathInfoServiceWrapper<PS, NS>
where
    PS: Deref<Target = dyn PathInfoService> + Send + Sync + 'static,
    NS: NarCalculationService + Send + Sync + 'static,
{
    type ListStream = BoxStream<'static, tonic::Result<proto::PathInfo, Status>>;

    #[instrument(skip_all)]
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
                match self.path_info_service.get(digest).await {
                    Ok(None) => Err(Status::not_found("PathInfo not found")),
                    Ok(Some(path_info)) => Ok(Response::new(proto::PathInfo::from(path_info))),
                    Err(e) => {
                        warn!(err = %e, "failed to get PathInfo");
                        Err(e.into())
                    }
                }
            }
        }
    }

    #[instrument(skip_all)]
    async fn put(&self, request: Request<proto::PathInfo>) -> Result<Response<proto::PathInfo>> {
        let path_info_proto = request.into_inner();

        let path_info = PathInfo::try_from(path_info_proto)
            .map_err(|e| Status::invalid_argument(format!("Invalid path info: {e}")))?;

        // Store the PathInfo in the client. Clients MUST validate the data
        // they receive, so we don't validate additionally here.
        match self.path_info_service.put(path_info).await {
            Ok(path_info_new) => Ok(Response::new(proto::PathInfo::from(path_info_new))),
            Err(e) => {
                warn!(err = %e, "failed to put PathInfo");
                Err(e.into())
            }
        }
    }

    #[instrument(skip_all)]
    async fn calculate_nar(
        &self,
        request: Request<castorepb::Node>,
    ) -> Result<Response<proto::CalculateNarResponse>> {
        let root_node = request
            .into_inner()
            .try_into_anonymous_node()
            .map_err(|e| {
                warn!(err = %e, "invalid root node");
                Status::invalid_argument("invalid root node")
            })?;

        match self.nar_calculation_service.calculate_nar(&root_node).await {
            Ok((nar_size, nar_sha256)) => Ok(Response::new(proto::CalculateNarResponse {
                nar_size,
                nar_sha256: nar_sha256.to_vec().into(),
            })),
            Err(e) => {
                warn!(err = %e, "error during NAR calculation");
                Err(e.into())
            }
        }
    }

    #[instrument(skip_all, err)]
    async fn list(
        &self,
        _request: Request<proto::ListPathInfoRequest>,
    ) -> Result<Response<Self::ListStream>, Status> {
        let stream = Box::pin(
            self.path_info_service
                .list()
                .map_ok(proto::PathInfo::from)
                .map_err(|e| Status::internal(e.to_string())),
        );

        Ok(Response::new(Box::pin(stream)))
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
