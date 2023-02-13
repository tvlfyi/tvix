mod memory;
mod sled;

use crate::{proto, Error};

pub use self::memory::MemoryPathInfoService;
pub use self::sled::SledPathInfoService;

/// The base trait all PathInfo services need to implement.
/// This is a simple get and put of [proto::Directory], returning their digest.
pub trait PathInfoService {
    /// Retrieve a PathInfo message.
    fn get(
        &self,
        by_what: proto::get_path_info_request::ByWhat,
    ) -> Result<Option<proto::PathInfo>, Error>;

    /// Store a PathInfo message. Implementations MUST call validate and reject
    /// invalid messages.
    fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, Error>;
}
