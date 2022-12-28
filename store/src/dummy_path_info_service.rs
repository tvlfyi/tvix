use crate::proto::path_info_service_server::PathInfoService;
use crate::proto::CalculateNarResponse;
use crate::proto::GetPathInfoRequest;
use crate::proto::Node;
use crate::proto::PathInfo;
use tonic::{Request, Response, Result, Status};
use tracing::{instrument, warn};

pub struct DummyPathInfoService {}

const NOT_IMPLEMENTED_MSG: &str = "not implemented";

#[tonic::async_trait]
impl PathInfoService for DummyPathInfoService {
    #[instrument(skip(self))]
    async fn get(&self, _request: Request<GetPathInfoRequest>) -> Result<Response<PathInfo>> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
    }

    #[instrument(skip(self))]
    async fn put(&self, _request: Request<PathInfo>) -> Result<Response<PathInfo>> {
        warn!(NOT_IMPLEMENTED_MSG);
        Err(Status::unimplemented(NOT_IMPLEMENTED_MSG))
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
