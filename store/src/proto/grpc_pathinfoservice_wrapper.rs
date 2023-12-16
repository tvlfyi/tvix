use crate::nar::RenderError;
use crate::pathinfoservice::PathInfoService;
use crate::proto;
use futures::StreamExt;
use std::ops::Deref;
use tokio::task;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{async_trait, Request, Response, Result, Status};
use tracing::{debug, instrument, warn};
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
    type ListStream = ReceiverStream<tonic::Result<proto::PathInfo, Status>>;

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
                match self.inner.get(digest).await {
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
        match self.inner.put(path_info).await {
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
        request: Request<castorepb::Node>,
    ) -> Result<Response<proto::CalculateNarResponse>> {
        match request.into_inner().node {
            None => Err(Status::invalid_argument("no root node sent")),
            Some(root_node) => {
                let (nar_size, nar_sha256) = self
                    .inner
                    .calculate_nar(&root_node)
                    .await
                    .expect("error during nar calculation"); // TODO: handle error

                Ok(Response::new(proto::CalculateNarResponse {
                    nar_size,
                    nar_sha256: nar_sha256.to_vec().into(),
                }))
            }
        }
    }

    #[instrument(skip(self))]
    async fn list(
        &self,
        _request: Request<proto::ListPathInfoRequest>,
    ) -> Result<Response<Self::ListStream>, Status> {
        let (tx, rx) = tokio::sync::mpsc::channel(5);

        let mut stream = self.inner.list();

        let _task = task::spawn(async move {
            while let Some(e) = stream.next().await {
                let res = e.map_err(|e| Status::internal(e.to_string()));
                if tx.send(res).await.is_err() {
                    debug!("receiver dropped");
                    break;
                }
            }
        });

        let receiver_stream = ReceiverStream::new(rx);
        Ok(Response::new(receiver_stream))
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
