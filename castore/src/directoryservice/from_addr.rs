use std::sync::Arc;
use url::Url;

use super::{DirectoryService, GRPCDirectoryService, MemoryDirectoryService, SledDirectoryService};

/// Constructs a new instance of a [DirectoryService] from an URI.
///
/// The following URIs are supported:
/// - `memory:`
///   Uses a in-memory implementation.
/// - `sled:`
///   Uses a in-memory sled implementation.
/// - `sled:///absolute/path/to/somewhere`
///   Uses sled, using a path on the disk for persistency. Can be only opened
///   from one process at the same time.
/// - `grpc+unix:///absolute/path/to/somewhere`
///   Connects to a local tvix-store gRPC service via Unix socket.
/// - `grpc+http://host:port`, `grpc+https://host:port`
///    Connects to a (remote) tvix-store gRPC service.
pub fn from_addr(uri: &str) -> Result<Arc<dyn DirectoryService>, crate::Error> {
    let url = Url::parse(uri)
        .map_err(|e| crate::Error::StorageError(format!("unable to parse url: {}", e)))?;

    Ok(if url.scheme() == "memory" {
        Arc::new(MemoryDirectoryService::from_url(&url)?)
    } else if url.scheme() == "sled" {
        Arc::new(SledDirectoryService::from_url(&url)?)
    } else if url.scheme().starts_with("grpc+") {
        Arc::new(GRPCDirectoryService::from_url(&url)?)
    } else {
        Err(crate::Error::StorageError(format!(
            "unknown scheme: {}",
            url.scheme()
        )))?
    })
}
