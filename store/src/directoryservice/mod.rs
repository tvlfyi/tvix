use crate::{proto, Error};
mod grpc;
mod memory;
mod sled;
mod utils;

pub use self::grpc::GRPCDirectoryService;
pub use self::memory::MemoryDirectoryService;
pub use self::sled::SledDirectoryService;
pub use self::utils::DirectoryTraverser;

/// The base trait all Directory services need to implement.
/// This is a simple get and put of [crate::proto::Directory], returning their
/// digest.
pub trait DirectoryService {
    type DirectoriesIterator: Iterator<Item = Result<proto::Directory, Error>> + Send;

    /// Get looks up a single Directory message by its digest.
    /// In case the directory is not found, Ok(None) is returned.
    fn get(&self, digest: &[u8; 32]) -> Result<Option<proto::Directory>, Error>;
    /// Get uploads a single Directory message, and returns the calculated
    /// digest, or an error.
    fn put(&self, directory: proto::Directory) -> Result<[u8; 32], Error>;

    /// Looks up a closure of [proto::Directory].
    /// Ideally this would be a `impl Iterator<Item = Result<proto::Directory, Error>>`,
    /// and we'd be able to add a default implementation for it here, but
    /// we can't have that yet.
    fn get_recursive(&self, root_directory_digest: &[u8; 32]) -> Self::DirectoriesIterator;
}
