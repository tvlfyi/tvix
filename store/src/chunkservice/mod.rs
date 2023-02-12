pub mod memory;
pub mod sled;

use crate::Error;

pub use self::memory::MemoryChunkService;
pub use self::sled::SledChunkService;

/// The base trait all ChunkService services need to implement.
/// It allows checking for the existence, download and upload of chunks.
/// It's usually used after consulting a [crate::blobservice::BlobService] for
/// chunking information.
pub trait ChunkService {
    /// check if the service has a chunk, given by its digest.
    fn has(&self, digest: &[u8]) -> Result<bool, Error>;

    /// retrieve a chunk by its digest. Implementations MUST validate the digest
    /// matches.
    fn get(&self, digest: &[u8]) -> Result<Option<Vec<u8>>, Error>;

    /// insert a chunk. returns the digest of the chunk, or an error.
    fn put(&self, data: Vec<u8>) -> Result<Vec<u8>, Error>;
}
