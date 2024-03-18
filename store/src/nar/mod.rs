use tvix_castore::B3Digest;

mod import;
mod renderer;
pub use import::read_nar;
pub use renderer::calculate_size_and_sha256;
pub use renderer::write_nar;

/// Errors that can encounter while rendering NARs.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("failure talking to a backing store client: {0}")]
    StoreError(#[source] std::io::Error),

    #[error("unable to find directory {}, referred from {:?}", .0, .1)]
    DirectoryNotFound(B3Digest, bytes::Bytes),

    #[error("unable to find blob {}, referred from {:?}", .0, .1)]
    BlobNotFound(B3Digest, bytes::Bytes),

    #[error("unexpected size in metadata for blob {}, referred from {:?} returned, expected {}, got {}", .0, .1, .2, .3)]
    UnexpectedBlobMeta(B3Digest, bytes::Bytes, u32, u32),

    #[error("failure using the NAR writer: {0}")]
    NARWriterError(std::io::Error),
}
