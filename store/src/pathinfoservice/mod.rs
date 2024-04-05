mod from_addr;
mod grpc;
mod memory;
mod nix_http;
mod sled;

#[cfg(any(feature = "fuse", feature = "virtiofs"))]
mod fs;

#[cfg(test)]
mod tests;

use futures::stream::BoxStream;
use tonic::async_trait;
use tvix_castore::proto as castorepb;
use tvix_castore::Error;

use crate::proto::PathInfo;

pub use self::from_addr::from_addr;
pub use self::grpc::GRPCPathInfoService;
pub use self::memory::MemoryPathInfoService;
pub use self::nix_http::NixHTTPPathInfoService;
pub use self::sled::SledPathInfoService;

#[cfg(feature = "cloud")]
mod bigtable;
#[cfg(feature = "cloud")]
pub use self::bigtable::BigtablePathInfoService;

#[cfg(any(feature = "fuse", feature = "virtiofs"))]
pub use self::fs::make_fs;

/// The base trait all PathInfo services need to implement.
#[async_trait]
pub trait PathInfoService: Send + Sync {
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
    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>>;
}

#[async_trait]
impl<A> PathInfoService for A
where
    A: AsRef<dyn PathInfoService> + Send + Sync,
{
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        self.as_ref().get(digest).await
    }

    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error> {
        self.as_ref().put(path_info).await
    }

    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), Error> {
        self.as_ref().calculate_nar(root_node).await
    }

    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
        self.as_ref().list()
    }
}
