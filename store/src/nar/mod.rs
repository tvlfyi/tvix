use data_encoding::BASE64;
use thiserror::Error;

mod renderer;

pub use renderer::NARRenderer;

/// Errors that can encounter while rendering NARs.
#[derive(Debug, Error)]
pub enum RenderError {
    #[error("failure talking to a backing store client: {0}")]
    StoreError(crate::Error),

    #[error("unable to find directory {}, referred from {}", BASE64.encode(.0), .1)]
    DirectoryNotFound(Vec<u8>, String),

    #[error("unable to find blob {}, referred from {}", BASE64.encode(.0), .1)]
    BlobNotFound(Vec<u8>, String),

    #[error("unexpected size in metadata for blob {}, referred from {} returned, expected {}, got {}", BASE64.encode(.0), .1, .2, .3)]
    UnexpectedBlobMeta(Vec<u8>, String, u32, u32),

    #[error("failure using the NAR writer: {0}")]
    NARWriterError(std::io::Error),
}
