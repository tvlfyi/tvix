use crate::{proto, Error};
mod memory;
mod sled;

pub use self::memory::MemoryDirectoryService;
pub use self::sled::SledDirectoryService;

/// The base trait all Directory services need to implement.
/// This is a simple get and put of [crate::proto::Directory], returning their
/// digest.
pub trait DirectoryService {
    /// Get looks up a single Directory message by its digest.
    /// In case the directory is not found, Ok(None) is returned.
    fn get(
        &self,
        by_what: &proto::get_directory_request::ByWhat,
    ) -> Result<Option<proto::Directory>, Error>;
    /// Get uploads a single Directory message, and returns the calculated
    /// digest, or an error.
    fn put(&self, directory: proto::Directory) -> Result<Vec<u8>, Error>;
}
