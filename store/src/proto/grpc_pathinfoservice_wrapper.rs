use crate::nar::RenderError;
use crate::pathinfoservice::PathInfoService;
use crate::proto;
use futures::{stream::BoxStream, TryStreamExt};
use std::ops::Deref;
use tonic::{async_trait, Request, Response, Result, Status};
use tracing::{instrument, warn};
use tvix_castore::proto as castorepb;

pub struct GRPCPathInfoServiceWrapper<PS> {
    inner: PS,
    // FUTUREWORK: allow exposing without allowing listing
}

impl<PS> GRPCPathInfoServiceWrapper<PS> {
    pub fn new(path_info_service: PS) -> Self {
        Self {
            inner: path_info_service,
        }
    }
}

#[async_trait]
impl<PS> proto::path_info_service_server::PathInfoService for GRPCPathInfoServiceWrapper<PS>
where
    PS: Deref<Target = dyn PathInfoService> + Send + Sync + 'static,
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
                match self.inner.get(digest).await {
                    Ok(None) => Err(Status::not_found("PathInfo not found")),
                    Ok(Some(path_info)) => Ok(Response::new(path_info)),
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
        let path_info = request.into_inner();

        // Store the PathInfo in the client. Clients MUST validate the data
        // they receive, so we don't validate additionally here.
        match self.inner.put(path_info).await {
            Ok(path_info_new) => Ok(Response::new(path_info_new)),
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
        match request.into_inner().node {
            None => Err(Status::invalid_argument("no root node sent")),
            Some(root_node) => {
                if let Err(e) = root_node.validate() {
                    warn!(err = %e, "invalid root node");
                    Err(Status::invalid_argument("invalid root node"))?
                }

                match self.inner.calculate_nar(&root_node).await {
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
        }
    }

    #[instrument(skip_all, err)]
    async fn list(
        &self,
        _request: Request<proto::ListPathInfoRequest>,
    ) -> Result<Response<Self::ListStream>, Status> {
        let stream = Box::pin(
            self.inner
                .list()
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
