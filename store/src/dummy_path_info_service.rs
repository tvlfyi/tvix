use crate::proto::path_info_service_server::PathInfoService;
use crate::proto::CalculateNarResponse;
use crate::proto::GetPathInfoRequest;
use crate::proto::Node;
use crate::proto::PathInfo;
use tonic::{Request, Response, Result, Status};

pub struct DummyPathInfoService {}

#[tonic::async_trait]
impl PathInfoService for DummyPathInfoService {
    async fn get(&self, _request: Request<GetPathInfoRequest>) -> Result<Response<PathInfo>> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn put(&self, _request: Request<PathInfo>) -> Result<Response<PathInfo>> {
        Err(Status::unimplemented("not implemented"))
    }

    async fn calculate_nar(
        &self,
        _request: Request<Node>,
    ) -> Result<Response<CalculateNarResponse>> {
        Err(Status::unimplemented("not implemented"))
    }
}
