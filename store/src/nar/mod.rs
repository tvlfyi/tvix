use tonic::async_trait;
use tvix_castore::B3Digest;

mod import;
mod renderer;
pub use import::ingest_nar;
pub use import::ingest_nar_and_hash;
pub use renderer::calculate_size_and_sha256;
pub use renderer::write_nar;
pub use renderer::SimpleRenderer;
use tvix_castore::proto as castorepb;

#[async_trait]
pub trait NarCalculationService: Send + Sync {
    /// Return the nar size and nar sha256 digest for a given root node.
    /// This can be used to calculate NAR-based output paths.
    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), tvix_castore::Error>;
}

#[async_trait]
impl<A> NarCalculationService for A
where
    A: AsRef<dyn NarCalculationService> + Send + Sync,
{
    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), tvix_castore::Error> {
        self.as_ref().calculate_nar(root_node).await
    }
}

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
