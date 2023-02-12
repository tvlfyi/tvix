use crate::{proto, Error};

mod memory;
mod sled;

pub use self::memory::MemoryBlobService;
pub use self::sled::SledBlobService;

/// The base trait all BlobService services need to implement.
/// It provides information about how a blob is chunked,
/// and allows creating new blobs by creating a BlobMeta (referring to chunks
/// in a [crate::chunkservice::ChunkService]).
pub trait BlobService {
    /// Retrieve chunking information for a given blob
    fn stat(&self, req: &proto::StatBlobRequest) -> Result<Option<proto::BlobMeta>, Error>;

    /// Insert chunking information for a given blob.
    /// Implementations SHOULD make sure chunks referred do exist.
    fn put(&self, blob_digest: &[u8], blob_meta: proto::BlobMeta) -> Result<(), Error>;
}
