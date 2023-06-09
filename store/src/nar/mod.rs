use crate::B3Digest;
use data_encoding::BASE64;
use thiserror::Error;

mod renderer;
pub use renderer::calculate_size_and_sha256;
pub use renderer::write_nar;

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
