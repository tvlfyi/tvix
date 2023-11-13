use crate::proto::path_info_service_client::PathInfoServiceClient;

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
        // memory doesn't support host or path in the URL.
        if url.has_host() || !url.path().is_empty() {
            return Err(Error::StorageError("invalid url".to_string()));
        }
        Arc::new(MemoryPathInfoService::new(blob_service, directory_service))
    } else if url.scheme() == "sled" {
        // sled doesn't support host, and a path can be provided (otherwise
        // it'll live in memory only).
        if url.has_host() {
            return Err(Error::StorageError("no host allowed".to_string()));
        }

        if url.path() == "/" {
            return Err(Error::StorageError(
                "cowardly refusing to open / with sled".to_string(),
            ));
        }

        // TODO: expose compression and other parameters as URL parameters?

        if url.path().is_empty() {
            return Ok(Arc::new(
                SledPathInfoService::new_temporary(blob_service, directory_service)
                    .map_err(|e| Error::StorageError(e.to_string()))?,
            ));
        }
        return Ok(Arc::new(
            SledPathInfoService::new(url.path().into(), blob_service, directory_service)
                .map_err(|e| Error::StorageError(e.to_string()))?,
        ));
    } else if url.scheme().starts_with("grpc+") {
        // schemes starting with grpc+ go to the GRPCPathInfoService.
        //   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
        // - In the case of unix sockets, there must be a path, but may not be a host.
        // - In the case of non-unix sockets, there must be a host, but no path.
        // Constructing the channel is handled by tvix_castore::channel::from_url.
        let client = PathInfoServiceClient::new(tvix_castore::channel::from_url(&url)?);
        Arc::new(GRPCPathInfoService::from_client(client))
    } else {
        Err(Error::StorageError(format!(
            "unknown scheme: {}",
            url.scheme()
        )))?
    })
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use tvix_castore::utils::{gen_blob_service, gen_directory_service};

    use super::from_addr;

    /// This uses a wrong scheme.
    #[test]
    fn invalid_scheme() {
        assert!(from_addr(
            "http://foo.example/test",
            gen_blob_service(),
            gen_directory_service()
        )
        .is_err());
    }

    /// This correctly sets the scheme, and doesn't set a path.
    #[test]
    fn memory_valid_scheme() {
        assert!(from_addr("memory://", gen_blob_service(), gen_directory_service()).is_ok())
    }

    /// This sets a memory url host to `foo`
    #[test]
    fn memory_invalid_host() {
        assert!(from_addr("memory://foo", gen_blob_service(), gen_directory_service()).is_err())
    }

    /// This sets a memory urlp path to "/", which is invalid.
    #[test]
    fn memory_invalid_has_path() {
        assert!(from_addr("memory:///", gen_blob_service(), gen_directory_service()).is_err())
    }

    /// This sets a memory url path "/foo", which is invalid.
    #[test]
    fn memory_invalid_path2() {
        assert!(from_addr("memory:///foo", gen_blob_service(), gen_directory_service()).is_err())
    }

    /// This uses the correct scheme, and doesn't specify a path (temporary sled).
    #[test]
    fn sled_valid_temporary() {
        assert!(from_addr("sled://", gen_blob_service(), gen_directory_service()).is_ok())
    }

    /// This uses the correct scheme, and sets a tempdir as location.
    #[test]
    fn sled_valid_scheme_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://").expect("must parse");
        url.set_path(tmpdir.path().to_str().unwrap());

        assert!(from_addr(
            &url.to_string(),
            gen_blob_service(),
            gen_directory_service()
        )
        .is_ok())
    }

    /// This uses the correct scheme, and specifies / as path (which should fail
    // for obvious reasons)
    #[test]
    fn sled_fail_invalid_path_root() {
        assert!(from_addr("sled:///", gen_blob_service(), gen_directory_service()).is_err())
    }

    /// This sets a host, rather than a path, which should fail.
    #[test]
    fn sled_invalid_host() {
        assert!(from_addr(
            "sled://foo.example",
            gen_blob_service(),
            gen_directory_service()
        )
        .is_err())
    }

    /// This sets a host AND a valid path, which should fail
    #[test]
    fn test_invalid_host_and_path() {
        let tmpdir = TempDir::new().unwrap();

        let mut url = url::Url::parse("sled://foo.example").expect("must parse");
        url.set_path(tmpdir.path().to_str().unwrap());

        assert!(from_addr(
            &url.to_string(),
            gen_blob_service(),
            gen_directory_service()
        )
        .is_err())
    }

    #[tokio::test]
    /// This constructs a GRPC PathInfoService.
    async fn grpc_valid() {
        assert!(from_addr(
            "grpc+http://[::1]:12345",
            gen_blob_service(),
            gen_directory_service()
        )
        .is_ok())
    }
}
