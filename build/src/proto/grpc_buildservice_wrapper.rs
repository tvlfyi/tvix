use crate::buildservice::BuildService;
use std::ops::Deref;
use tonic::async_trait;

use super::{Build, BuildRequest};

/// Implements the gRPC server trait ([crate::proto::build_service_server::BuildService]
/// for anything implementing [BuildService].
pub struct GRPCBuildServiceWrapper<BUILD> {
    inner: BUILD,
}

impl<BUILD> GRPCBuildServiceWrapper<BUILD> {
    pub fn new(build_service: BUILD) -> Self {
        Self {
            inner: build_service,
        }
    }
}

#[async_trait]
impl<BUILD> crate::proto::build_service_server::BuildService for GRPCBuildServiceWrapper<BUILD>
where
    BUILD: Deref<Target = dyn BuildService> + Send + Sync + 'static,
{
    async fn do_build(
        &self,
        request: tonic::Request<BuildRequest>,
    ) -> Result<tonic::Response<Build>, tonic::Status> {
        let request = TryInto::<crate::buildservice::BuildRequest>::try_into(request.into_inner())
            .map_err(|err| tonic::Status::new(tonic::Code::InvalidArgument, err.to_string()))?;
        match self.inner.do_build(request).await {
            Ok(resp) => Ok(tonic::Response::new(resp)),
            Err(e) => Err(tonic::Status::internal(e.to_string())),
        }
    }
}
