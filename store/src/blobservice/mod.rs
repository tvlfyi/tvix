use std::io;

use crate::Error;

mod memory;
mod sled;

pub use self::memory::MemoryBlobService;
pub use self::sled::SledBlobService;

/// The base trait all BlobService services need to implement.
/// It provides functions to check whether a given blob exists,
/// a way to get a [io::Read] to a blob, and a method to initiate writing a new
/// Blob, which returns a [BlobWriter], that can be used
pub trait BlobService {
    type BlobReader: io::Read + Send + std::marker::Unpin;
    type BlobWriter: BlobWriter + Send;

    /// Check if the service has the blob, by its content hash.
    fn has(&self, digest: &[u8; 32]) -> Result<bool, Error>;

    /// Request a blob from the store, by its content hash. Returns a Option<BlobReader>.
    fn open_read(&self, digest: &[u8; 32]) -> Result<Option<Self::BlobReader>, Error>;

    /// Insert a new blob into the store. Returns a [BlobWriter], which
    /// implements [io::Write] and a [BlobWriter::close].
    /// TODO: is there any reason we want this to be a Result<>, and not just T?
    fn open_write(&self) -> Result<Self::BlobWriter, Error>;
}

/// A [io::Write] that you need to close() afterwards, and get back the digest
/// of the written blob.
pub trait BlobWriter: io::Write {
    /// Signal there's no more data to be written, and return the digest of the
    /// contents written.
    ///
    /// This consumes self, so it's not possible to close twice.
    fn close(self) -> Result<[u8; 32], Error>;
}
