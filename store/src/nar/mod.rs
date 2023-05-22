use crate::{proto, B3Digest};
use data_encoding::BASE64;
use thiserror::Error;

mod grpc_nar_calculation_service;
mod non_caching_calculation_service;
mod renderer;

pub use grpc_nar_calculation_service::GRPCNARCalculationService;
pub use non_caching_calculation_service::NonCachingNARCalculationService;
pub use renderer::NARRenderer;

/// Errors that can encounter while rendering NARs.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("failure talking to a backing store client: {0}")]
    StoreError(crate::Error),

    #[error("unable to find directory {}, referred from {}", .0, .1)]
    DirectoryNotFound(B3Digest, String),

    #[error("unable to find blob {}, referred from {}", BASE64.encode(.0), .1)]
    BlobNotFound([u8; 32], String),

    #[error("unexpected size in metadata for blob {}, referred from {} returned, expected {}, got {}", BASE64.encode(.0), .1, .2, .3)]
    UnexpectedBlobMeta([u8; 32], String, u32, u32),

    #[error("failure using the NAR writer: {0}")]
    NARWriterError(std::io::Error),
}

/// The base trait for something calculating NARs, and returning their size and sha256.
pub trait NARCalculationService {
    fn calculate_nar(&self, root_node: &proto::node::Node) -> Result<(u64, [u8; 32]), RenderError>;
}
