use std::sync::Arc;
use url::Url;

use super::{BlobService, GRPCBlobService, MemoryBlobService, SledBlobService};

/// Constructs a new instance of a [BlobService] from an URI.
///
/// The following schemes are supported by the following services:
/// - `memory://` ([MemoryBlobService])
/// - `sled://` ([SledBlobService])
/// - `grpc+*://` ([GRPCBlobService])
///
/// See their [from_url] methods for more details about their syntax.
pub fn from_addr(uri: &str) -> Result<Arc<dyn BlobService>, crate::Error> {
    let url = Url::parse(uri)
        .map_err(|e| crate::Error::StorageError(format!("unable to parse url: {}", e)))?;

    Ok(if url.scheme() == "memory" {
        Arc::new(MemoryBlobService::from_url(&url)?)
    } else if url.scheme() == "sled" {
        Arc::new(SledBlobService::from_url(&url)?)
    } else if url.scheme().starts_with("grpc+") {
        Arc::new(GRPCBlobService::from_url(&url)?)
    } else {
        Err(crate::Error::StorageError(format!(
            "unknown scheme: {}",
            url.scheme()
        )))?
    })
}
