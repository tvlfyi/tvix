use crate::{proto, B3Digest, Error};
use futures::Stream;
use std::pin::Pin;
use tonic::async_trait;

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

/// The base trait all Directory services need to implement.
/// This is a simple get and put of [crate::proto::Directory], returning their
/// digest.
#[async_trait]
pub trait DirectoryService: Send + Sync {
    /// Create a new instance by passing in a connection URL.
    /// TODO: check if we want to make this async, instead of lazily connecting
    fn from_url(url: &url::Url) -> Result<Self, Error>
    where
        Self: Sized;

    /// Get looks up a single Directory message by its digest.
    /// In case the directory is not found, Ok(None) is returned.
    async fn get(&self, digest: &B3Digest) -> Result<Option<proto::Directory>, Error>;
    /// Get uploads a single Directory message, and returns the calculated
    /// digest, or an error.
    async fn put(&self, directory: proto::Directory) -> Result<B3Digest, Error>;

    /// Looks up a closure of [proto::Directory].
    /// Ideally this would be a `impl Stream<Item = Result<proto::Directory, Error>>`,
    /// and we'd be able to add a default implementation for it here, but
    /// we can't have that yet.
    ///
    /// This returns a pinned, boxed stream. The pinning allows for it to be polled easily,
    /// and the box allows different underlying stream implementations to be returned since
    /// Rust doesn't support this as a generic in traits yet. This is the same thing that
    /// [async_trait] generates, but for streams instead of futures.
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> Pin<Box<dyn Stream<Item = Result<proto::Directory, Error>> + Send>>;

    /// Allows persisting a closure of [proto::Directory], which is a graph of
    /// connected Directory messages.
    fn put_multiple_start(&self) -> Box<dyn DirectoryPutter>;
}

/// Provides a handle to put a closure of connected [proto::Directory] elements.
///
/// The consumer can periodically call [DirectoryPutter::put], starting from the
/// leaves. Once the root is reached, [DirectoryPutter::close] can be called to
/// retrieve the root digest (or an error).
#[async_trait]
pub trait DirectoryPutter: Send {
    /// Put a individual [proto::Directory] into the store.
    /// Error semantics and behaviour is up to the specific implementation of
    /// this trait.
    /// Due to bursting, the returned error might refer to an object previously
    /// sent via `put`.
    async fn put(&mut self, directory: proto::Directory) -> Result<(), Error>;

    /// Close the stream, and wait for any errors.
    async fn close(&mut self) -> Result<B3Digest, Error>;

    /// Return whether the stream is closed or not.
    /// Used from some [DirectoryService] implementations only.
    fn is_closed(&self) -> bool;
}
