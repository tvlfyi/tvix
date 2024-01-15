use tonic::async_trait;

use crate::proto::{Build, BuildRequest};

mod dummy;
pub use dummy::DummyBuildService;

#[async_trait]
pub trait BuildService: Send + Sync {
    /// TODO: document
    async fn do_build(&self, request: BuildRequest) -> std::io::Result<Build>;
}
