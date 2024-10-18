mod combinators;
mod from_addr;
mod grpc;
mod lru;
mod memory;
mod nix_http;
mod redb;
mod signing_wrapper;

#[cfg(any(feature = "fuse", feature = "virtiofs"))]
mod fs;

#[cfg(test)]
mod tests;

use auto_impl::auto_impl;
use futures::stream::BoxStream;
use tonic::async_trait;
use tvix_castore::composition::{Registry, ServiceBuilder};
use tvix_castore::Error;

use crate::nar::NarCalculationService;
pub use crate::path_info::PathInfo;

pub use self::combinators::{
    Cache as CachePathInfoService, CacheConfig as CachePathInfoServiceConfig,
};
pub use self::from_addr::from_addr;
pub use self::grpc::{GRPCPathInfoService, GRPCPathInfoServiceConfig};
pub use self::lru::{LruPathInfoService, LruPathInfoServiceConfig};
pub use self::memory::{MemoryPathInfoService, MemoryPathInfoServiceConfig};
pub use self::nix_http::{NixHTTPPathInfoService, NixHTTPPathInfoServiceConfig};
pub use self::redb::{RedbPathInfoService, RedbPathInfoServiceConfig};
pub use self::signing_wrapper::{KeyFileSigningPathInfoServiceConfig, SigningPathInfoService};

#[cfg(test)]
pub(crate) use self::signing_wrapper::test_signing_service;

#[cfg(feature = "cloud")]
mod bigtable;
#[cfg(feature = "cloud")]
pub use self::bigtable::{BigtableParameters, BigtablePathInfoService};

#[cfg(any(feature = "fuse", feature = "virtiofs"))]
pub use self::fs::make_fs;

/// The base trait all PathInfo services need to implement.
#[async_trait]
#[auto_impl(&, &mut, Arc, Box)]
pub trait PathInfoService: Send + Sync {
    /// Retrieve a PathInfo message by the output digest.
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error>;

    /// Store a PathInfo message. Implementations MUST call validate and reject
    /// invalid messages.
    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error>;

    /// Iterate over all PathInfo objects in the store.
    /// Implementations can decide to disallow listing.
    ///
    /// This returns a pinned, boxed stream. The pinning allows for it to be polled easily,
    /// and the box allows different underlying stream implementations to be returned since
    /// Rust doesn't support this as a generic in traits yet. This is the same thing that
    /// [async_trait] generates, but for streams instead of futures.
    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>>;

    /// Returns a (more) suitable NarCalculationService.
    /// This can be used to offload NAR calculation to the remote side.
    fn nar_calculation_service(&self) -> Option<Box<dyn NarCalculationService>> {
        None
    }
}

/// Registers the builtin PathInfoService implementations with the registry
pub(crate) fn register_pathinfo_services(reg: &mut Registry) {
    reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, CachePathInfoServiceConfig>("cache");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, GRPCPathInfoServiceConfig>("grpc");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, LruPathInfoServiceConfig>("lru");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, MemoryPathInfoServiceConfig>("memory");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, NixHTTPPathInfoServiceConfig>("nix");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, RedbPathInfoServiceConfig>("redb");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, KeyFileSigningPathInfoServiceConfig>("keyfile-signing");
    #[cfg(feature = "cloud")]
    {
        reg.register::<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>, BigtableParameters>(
            "bigtable",
        );
    }
}
