use tonic::{async_trait, transport::Channel};

use crate::buildservice::BuildRequest;
use crate::proto::{self, build_service_client::BuildServiceClient};

use super::BuildService;

pub struct GRPCBuildService {
    client: BuildServiceClient<Channel>,
}

impl GRPCBuildService {
    #[allow(dead_code)]
    pub fn from_client(client: BuildServiceClient<Channel>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl BuildService for GRPCBuildService {
    async fn do_build(&self, request: BuildRequest) -> std::io::Result<proto::Build> {
        let mut client = self.client.clone();
        client
            .do_build(Into::<proto::BuildRequest>::into(request))
            .await
            .map(|resp| resp.into_inner())
            .map_err(std::io::Error::other)
    }
}
