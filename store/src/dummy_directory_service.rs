use tokio_stream::wrappers::ReceiverStream;

use crate::proto::directory_service_server::DirectoryService;
use crate::proto::Directory;
use crate::proto::GetDirectoryRequest;
use crate::proto::PutDirectoryResponse;
use tonic::{Request, Response, Result, Status, Streaming};

pub struct DummyDirectoryService {}

#[tonic::async_trait]
impl DirectoryService for DummyDirectoryService {
    type GetStream = ReceiverStream<Result<Directory>>;

    async fn get(
        &self,
        _request: Request<GetDirectoryRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn put(
        &self,
        _request: Request<Streaming<Directory>>,
    ) -> Result<Response<PutDirectoryResponse>> {
        Err(Status::unimplemented("not implemented"))
    }
}
