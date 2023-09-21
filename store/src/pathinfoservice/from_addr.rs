use super::{GRPCPathInfoService, MemoryPathInfoService, PathInfoService, SledPathInfoService};

use std::sync::Arc;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService, Error};
use url::Url;

/// Constructs a new instance of a [PathInfoService] from an URI.
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
///
/// As the [PathInfoService] needs to talk to [BlobService] and [DirectoryService],
/// these also need to be passed in.
pub fn from_addr(
    uri: &str,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<Arc<dyn PathInfoService>, Error> {
    let url =
        Url::parse(uri).map_err(|e| Error::StorageError(format!("unable to parse url: {}", e)))?;

    Ok(if url.scheme() == "memory" {
        Arc::new(MemoryPathInfoService::from_url(
            &url,
            blob_service,
            directory_service,
        )?)
    } else if url.scheme() == "sled" {
        Arc::new(SledPathInfoService::from_url(
            &url,
            blob_service,
            directory_service,
        )?)
    } else if url.scheme().starts_with("grpc+") {
        Arc::new(GRPCPathInfoService::from_url(
            &url,
            blob_service,
            directory_service,
        )?)
    } else {
        Err(Error::StorageError(format!(
            "unknown scheme: {}",
            url.scheme()
        )))?
    })
}
