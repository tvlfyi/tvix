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

    #[error("failure using the NAR writer: {0}")]
    NARWriterError(std::io::Error),
}
