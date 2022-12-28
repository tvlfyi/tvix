use tokio_stream::wrappers::ReceiverStream;

use crate::proto::directory_service_server::DirectoryService;
use crate::proto::Directory;
use crate::proto::GetDirectoryRequest;
use crate::proto::PutDirectoryResponse;
use tonic::{Request, Response, Result, Status, Streaming};
use tracing::{instrument, warn};

const NOT_IMPLEMENTED_MSG: &str = "not implemented";

pub struct DummyDirectoryService {}

#[tonic::async_trait]
impl DirectoryService for DummyDirectoryService {
    type GetStream = ReceiverStream<Result<Directory>>;

    #[instrument(skip(self))]
    async fn get(
        &self,
        _request: Request<GetDirectoryRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
    }

    #[instrument(skip(self, _request))]
    async fn put(
        &self,
        _request: Request<Streaming<Directory>>,
    ) -> Result<Response<PutDirectoryResponse>> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
    }
}
