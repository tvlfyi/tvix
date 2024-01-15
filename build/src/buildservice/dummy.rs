use tonic::async_trait;
use tracing::instrument;

use super::BuildService;
use crate::proto::{Build, BuildRequest};

#[derive(Default)]
pub struct DummyBuildService {}

#[async_trait]
impl BuildService for DummyBuildService {
    #[instrument(skip(self), ret, err)]
    async fn do_build(&self, _request: BuildRequest) -> std::io::Result<Build> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "builds are not supported with DummyBuildService",
        ))
    }
}
