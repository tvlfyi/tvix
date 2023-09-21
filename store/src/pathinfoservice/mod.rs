mod from_addr;
mod grpc;
mod memory;
mod sled;

use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use tonic::async_trait;
use tvix_castore::blobservice::BlobService;
use tvix_castore::directoryservice::DirectoryService;
use tvix_castore::proto as castorepb;
use tvix_castore::Error;

use crate::proto::PathInfo;

pub use self::from_addr::from_addr;
pub use self::grpc::GRPCPathInfoService;
pub use self::memory::MemoryPathInfoService;
pub use self::sled::SledPathInfoService;

/// The base trait all PathInfo services need to implement.
#[async_trait]
pub trait PathInfoService: Send + Sync {
    /// Create a new instance by passing in a connection URL, as well
    /// as instances of a [PathInfoService] and [DirectoryService] (as the
    /// [PathInfoService] needs to talk to them).
    /// TODO: check if we want to make this async, instead of lazily connecting
    fn from_url(
        url: &url::Url,
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) -> Result<Self, Error>
    where
        Self: Sized;

    /// Retrieve a PathInfo message by the output digest.
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error>;

    /// Store a PathInfo message. Implementations MUST call validate and reject
    /// invalid messages.
    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error>;

    /// Return the nar size and nar sha256 digest for a given root node.
    /// This can be used to calculate NAR-based output paths,
    /// and implementations are encouraged to cache it.
    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), Error>;

    /// Iterate over all PathInfo objects in the store.
    /// Implementations can decide to disallow listing.
    ///
    /// This returns a pinned, boxed stream. The pinning allows for it to be polled easily,
    /// and the box allows different underlying stream implementations to be returned since
    /// Rust doesn't support this as a generic in traits yet. This is the same thing that
    /// [async_trait] generates, but for streams instead of futures.
    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<PathInfo, Error>> + Send>>;
}
