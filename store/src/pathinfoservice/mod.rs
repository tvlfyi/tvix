mod from_addr;
mod grpc;
mod memory;
mod sled;

use std::sync::Arc;

use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryService;
use crate::{proto, Error};

pub use self::from_addr::from_addr;
pub use self::grpc::GRPCPathInfoService;
pub use self::memory::MemoryPathInfoService;
pub use self::sled::SledPathInfoService;

/// The base trait all PathInfo services need to implement.
/// This is a simple get and put of [proto::Directory], returning their digest.
pub trait PathInfoService: Send + Sync {
    /// Create a new instance by passing in a connection URL, as well
    /// as instances of a [PathInfoService] and [DirectoryService] (as the
    /// [PathInfoService] needs to talk to them).
    fn from_url(
        url: &url::Url,
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) -> Result<Self, Error>
    where
        Self: Sized;

    /// Retrieve a PathInfo message by the output digest.
    fn get(&self, digest: [u8; 20]) -> Result<Option<proto::PathInfo>, Error>;

    /// Store a PathInfo message. Implementations MUST call validate and reject
    /// invalid messages.
    fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, Error>;

    /// Return the nar size and nar sha256 digest for a given root node.
    /// This can be used to calculate NAR-based output paths,
    /// and implementations are encouraged to cache it.
    fn calculate_nar(&self, root_node: &proto::node::Node) -> Result<(u64, [u8; 32]), Error>;
}
