use std::io;

use crate::{B3Digest, Error};

mod grpc;
mod memory;
mod sled;

pub use self::grpc::GRPCBlobService;
pub use self::memory::MemoryBlobService;
pub use self::sled::SledBlobService;

/// The base trait all BlobService services need to implement.
/// It provides functions to check whether a given blob exists,
/// a way to get a [io::Read] to a blob, and a method to initiate writing a new
/// Blob, which will return something implmenting io::Write, and providing a
/// close funtion, to finalize a blob and get its digest.
pub trait BlobService: Send + Sync {
    /// Check if the service has the blob, by its content hash.
    fn has(&self, digest: &B3Digest) -> Result<bool, Error>;

    /// Request a blob from the store, by its content hash.
    fn open_read(&self, digest: &B3Digest) -> Result<Option<Box<dyn io::Read + Send>>, Error>;

    /// Insert a new blob into the store. Returns a [BlobWriter], which
    /// implements [io::Write] and a [BlobWriter::close].
    /// TODO: is there any reason we want this to be a Result<>, and not just T?
    fn open_write(&self) -> Result<Box<dyn BlobWriter>, Error>;
}

/// A [io::Write] that you need to close() afterwards, and get back the digest
/// of the written blob.
pub trait BlobWriter: io::Write + Send + Sync + 'static {
    /// Signal there's no more data to be written, and return the digest of the
    /// contents written.
    ///
    /// Closing a already-closed BlobWriter is a no-op.
    fn close(&mut self) -> Result<B3Digest, Error>;
}
