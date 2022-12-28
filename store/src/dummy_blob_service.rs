use tokio_stream::wrappers::ReceiverStream;

use crate::proto::blob_service_server::BlobService;
use crate::proto::BlobChunk;
use crate::proto::BlobMeta;
use crate::proto::PutBlobResponse;
use crate::proto::ReadBlobRequest;
use crate::proto::StatBlobRequest;
use tonic::{Request, Response, Result, Status, Streaming};
use tracing::{instrument, warn};

const NOT_IMPLEMENTED_MSG: &str = "not implemented";

pub struct DummyBlobService {}

#[tonic::async_trait]
impl BlobService for DummyBlobService {
    type ReadStream = ReceiverStream<Result<BlobChunk>>;

    #[instrument(skip(self))]
    async fn stat(&self, _request: Request<StatBlobRequest>) -> Result<Response<BlobMeta>> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
    }

    #[instrument(skip(self))]
    async fn read(
        &self,
        _request: Request<ReadBlobRequest>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
    }

    #[instrument(skip(self, _request))]
    async fn put(
        &self,
        _request: Request<Streaming<BlobChunk>>,
    ) -> Result<Response<PutBlobResponse>> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
    }
}
