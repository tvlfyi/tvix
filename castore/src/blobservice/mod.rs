use std::io;
use tonic::async_trait;

use crate::proto::stat_blob_response::ChunkMeta;
use crate::B3Digest;

mod combinator;
mod from_addr;
mod grpc;
mod memory;
mod naive_seeker;
mod simplefs;
mod sled;

#[cfg(test)]
mod tests;

pub use self::combinator::CombinedBlobService;
pub use self::from_addr::from_addr;
pub use self::grpc::GRPCBlobService;
pub use self::memory::MemoryBlobService;
pub use self::simplefs::SimpleFilesystemBlobService;
pub use self::sled::SledBlobService;

/// The base trait all BlobService services need to implement.
/// It provides functions to check whether a given blob exists,
/// a way to get a [io::Read] to a blob, and a method to initiate writing a new
/// Blob, which will return something implmenting io::Write, and providing a
/// close funtion, to finalize a blob and get its digest.
#[async_trait]
pub trait BlobService: Send + Sync {
    /// Check if the service has the blob, by its content hash.
    async fn has(&self, digest: &B3Digest) -> io::Result<bool>;

    /// Request a blob from the store, by its content hash.
    async fn open_read(&self, digest: &B3Digest) -> io::Result<Option<Box<dyn BlobReader>>>;

    /// Insert a new blob into the store. Returns a [BlobWriter], which
    /// implements [io::Write] and a [BlobWriter::close].
    async fn open_write(&self) -> Box<dyn BlobWriter>;

    /// Return a list of chunks for a given blob.
    /// There's a distinction between returning Ok(None) and Ok(Some(vec![])).
    /// The former return value is sent in case the blob is not present at all,
    /// while the second one is sent in case there's no more granular chunks (or
    /// the backend does not support chunking).
    /// A default implementation signalling the backend does not support
    /// chunking is provided.
    async fn chunks(&self, digest: &B3Digest) -> io::Result<Option<Vec<ChunkMeta>>> {
        if !self.has(digest).await? {
            return Ok(None);
        } else {
            // default implementation, signalling the backend does not support chunking.
            return Ok(Some(vec![]));
        }
    }
}

/// A [tokio::io::AsyncWrite] that you need to close() afterwards, and get back
/// the digest of the written blob.
#[async_trait]
pub trait BlobWriter: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static {
    /// Signal there's no more data to be written, and return the digest of the
    /// contents written.
    ///
    /// Closing a already-closed BlobWriter is a no-op.
    async fn close(&mut self) -> io::Result<B3Digest>;
}

/// A [tokio::io::AsyncRead] that also allows seeking.
pub trait BlobReader: tokio::io::AsyncRead + tokio::io::AsyncSeek + Send + Unpin + 'static {}

/// A [`io::Cursor<Vec<u8>>`] can be used as a BlobReader.
impl BlobReader for io::Cursor<Vec<u8>> {}
impl BlobReader for tokio::fs::File {}
