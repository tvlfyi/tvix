use tokio_stream::wrappers::ReceiverStream;

use crate::proto::blob_service_server::BlobService;
use crate::proto::BlobChunk;
use crate::proto::BlobMeta;
use crate::proto::PutBlobResponse;
use crate::proto::ReadBlobRequest;
use crate::proto::StatBlobRequest;
use tonic::{Request, Response, Result, Status, Streaming};

pub struct DummyBlobService {}

#[tonic::async_trait]
impl BlobService for DummyBlobService {
    type ReadStream = ReceiverStream<Result<BlobChunk>>;

    async fn stat(&self, _request: Request<StatBlobRequest>) -> Result<Response<BlobMeta>> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn read(
        &self,
        _request: Request<ReadBlobRequest>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn put(
        &self,
        _request: Request<Streaming<BlobChunk>>,
    ) -> Result<Response<PutBlobResponse>> {
        Err(Status::unimplemented("not implemented"))
    }
}
