mod grpc;
mod memory;
mod sled;

use crate::{proto, Error};

pub use self::grpc::GRPCPathInfoService;
pub use self::memory::MemoryPathInfoService;
pub use self::sled::SledPathInfoService;

/// The base trait all PathInfo services need to implement.
/// This is a simple get and put of [proto::Directory], returning their digest.
pub trait PathInfoService {
    /// Retrieve a PathInfo message by the output digest.
    fn get(&self, digest: [u8; 20]) -> Result<Option<proto::PathInfo>, Error>;

    /// Store a PathInfo message. Implementations MUST call validate and reject
    /// invalid messages.
    fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, Error>;

    // TODO: add default impl for nar calculation here, and override from GRPC client!
}
