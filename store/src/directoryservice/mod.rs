use crate::{proto, B3Digest, Error};

mod from_addr;
mod grpc;
mod memory;
mod sled;
mod traverse;
mod utils;

pub use self::from_addr::from_addr;
pub use self::grpc::GRPCDirectoryService;
pub use self::memory::MemoryDirectoryService;
pub use self::sled::SledDirectoryService;
pub use self::traverse::traverse_to;
pub use self::utils::DirectoryTraverser;

/// The base trait all Directory services need to implement.
/// This is a simple get and put of [crate::proto::Directory], returning their
/// digest.
pub trait DirectoryService: Send + Sync {
    /// Create a new instance by passing in a connection URL.
    fn from_url(url: &url::Url) -> Result<Self, Error>
    where
        Self: Sized;

    /// Get looks up a single Directory message by its digest.
    /// In case the directory is not found, Ok(None) is returned.
    fn get(&self, digest: &B3Digest) -> Result<Option<proto::Directory>, Error>;
    /// Get uploads a single Directory message, and returns the calculated
    /// digest, or an error.
    fn put(&self, directory: proto::Directory) -> Result<B3Digest, Error>;

    /// Looks up a closure of [proto::Directory].
    /// Ideally this would be a `impl Iterator<Item = Result<proto::Directory, Error>>`,
    /// and we'd be able to add a default implementation for it here, but
    /// we can't have that yet.
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> Box<dyn Iterator<Item = Result<proto::Directory, Error>> + Send>;

    /// Allows persisting a closure of [proto::Directory], which is a graph of
    /// connected Directory messages.
    fn put_multiple_start(&self) -> Box<dyn DirectoryPutter>;
}

/// Provides a handle to put a closure of connected [proto::Directory] elements.
///
/// The consumer can periodically call [put], starting from the leaves. Once
/// the root is reached, [close] can be called to retrieve the root digest (or
/// an error).
pub trait DirectoryPutter {
    /// Put a individual [proto::Directory] into the store.
    /// Error semantics and behaviour is up to the specific implementation of
    /// this trait.
    /// Due to bursting, the returned error might refer to an object previously
    /// sent via `put`.
    fn put(&mut self, directory: proto::Directory) -> Result<(), Error>;

    /// Close the stream, and wait for any errors.
    fn close(&mut self) -> Result<B3Digest, Error>;

    /// Return whether the stream is closed or not.
    /// Used from some [DirectoryService] implementations only.
    fn is_closed(&self) -> bool;
}
