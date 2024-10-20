use tonic::async_trait;

use crate::proto;

pub mod build_request;
pub use crate::buildservice::build_request::*;
mod dummy;
mod from_addr;
mod grpc;

#[cfg(target_os = "linux")]
mod oci;

pub use dummy::DummyBuildService;
pub use from_addr::from_addr;

#[async_trait]
pub trait BuildService: Send + Sync {
    /// TODO: document
    async fn do_build(&self, request: BuildRequest) -> std::io::Result<proto::Build>;
}
