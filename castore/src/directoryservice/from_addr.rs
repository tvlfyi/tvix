use url::Url;

use crate::{proto::directory_service_client::DirectoryServiceClient, Error};

use super::{
    DirectoryService, GRPCDirectoryService, MemoryDirectoryService, ObjectStoreDirectoryService,
    SledDirectoryService,
};

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
pub async fn from_addr(uri: &str) -> Result<Box<dyn DirectoryService>, crate::Error> {
    #[allow(unused_mut)]
    let mut url = Url::parse(uri)
        .map_err(|e| crate::Error::StorageError(format!("unable to parse url: {}", e)))?;

    let directory_service: Box<dyn DirectoryService> = match url.scheme() {
        "memory" => {
            // memory doesn't support host or path in the URL.
            if url.has_host() || !url.path().is_empty() {
                return Err(Error::StorageError("invalid url".to_string()));
            }
            Box::<MemoryDirectoryService>::default()
        }
        "sled" => {
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

            Box::new(if url.path().is_empty() {
                SledDirectoryService::new_temporary()
                    .map_err(|e| Error::StorageError(e.to_string()))?
            } else {
                SledDirectoryService::new(url.path())
                    .map_err(|e| Error::StorageError(e.to_string()))?
            })
        }
        scheme if scheme.starts_with("grpc+") => {
            // schemes starting with grpc+ go to the GRPCPathInfoService.
            //   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
            // - In the case of unix sockets, there must be a path, but may not be a host.
            // - In the case of non-unix sockets, there must be a host, but no path.
            // Constructing the channel is handled by tvix_castore::channel::from_url.
            let client = DirectoryServiceClient::new(crate::tonic::channel_from_url(&url).await?);
            Box::new(GRPCDirectoryService::from_client(client))
        }
        scheme if scheme.starts_with("objectstore+") => {
            // We need to convert the URL to string, strip the prefix there, and then
            // parse it back as url, as Url::set_scheme() rejects some of the transitions we want to do.
            let trimmed_url = {
                let s = url.to_string();
                Url::parse(s.strip_prefix("objectstore+").unwrap()).unwrap()
            };
            Box::new(
                ObjectStoreDirectoryService::parse_url(&trimmed_url)
                    .map_err(|e| Error::StorageError(e.to_string()))?,
            )
        }
        #[cfg(feature = "cloud")]
        "bigtable" => {
            use super::bigtable::BigtableParameters;
            use super::BigtableDirectoryService;

            // parse the instance name from the hostname.
            let instance_name = url
                .host_str()
                .ok_or_else(|| Error::StorageError("instance name missing".into()))?
                .to_string();

            // … but add it to the query string now, so we just need to parse that.
            url.query_pairs_mut()
                .append_pair("instance_name", &instance_name);

            let params: BigtableParameters = serde_qs::from_str(url.query().unwrap_or_default())
                .map_err(|e| Error::InvalidRequest(format!("failed to parse parameters: {}", e)))?;

            Box::new(
                BigtableDirectoryService::connect(params)
                    .await
                    .map_err(|e| Error::StorageError(e.to_string()))?,
            )
        }
        _ => {
            return Err(crate::Error::StorageError(format!(
                "unknown scheme: {}",
                url.scheme()
            )))
        }
    };
    Ok(directory_service)
}

#[cfg(test)]
mod tests {
    use super::from_addr;
    use lazy_static::lazy_static;
    use rstest::rstest;
    use tempfile::TempDir;

    lazy_static! {
        static ref TMPDIR_SLED_1: TempDir = TempDir::new().unwrap();
        static ref TMPDIR_SLED_2: TempDir = TempDir::new().unwrap();
    }

    #[rstest]
    /// This uses an unsupported scheme.
    #[case::unsupported_scheme("http://foo.example/test", false)]
    /// This configures sled in temporary mode.
    #[case::sled_valid_temporary("sled://", true)]
    /// This configures sled with /, which should fail.
    #[case::sled_invalid_root("sled:///", false)]
    /// This configures sled with a host, not path, which should fail.
    #[case::sled_invalid_host("sled://foo.example", false)]
    /// This configures sled with a valid path path, which should succeed.
    #[case::sled_valid_path(&format!("sled://{}", &TMPDIR_SLED_1.path().to_str().unwrap()), true)]
    /// This configures sled with a host, and a valid path path, which should fail.
    #[case::sled_invalid_host_with_valid_path(&format!("sled://foo.example{}", &TMPDIR_SLED_2.path().to_str().unwrap()), false)]
    /// This correctly sets the scheme, and doesn't set a path.
    #[case::memory_valid("memory://", true)]
    /// This sets a memory url host to `foo`
    #[case::memory_invalid_host("memory://foo", false)]
    /// This sets a memory url path to "/", which is invalid.
    #[case::memory_invalid_root_path("memory:///", false)]
    /// This sets a memory url path to "/foo", which is invalid.
    #[case::memory_invalid_root_path_foo("memory:///foo", false)]
    /// Correct scheme to connect to a unix socket.
    #[case::grpc_valid_unix_socket("grpc+unix:///path/to/somewhere", true)]
    /// Correct scheme for unix socket, but setting a host too, which is invalid.
    #[case::grpc_invalid_unix_socket_and_host("grpc+unix://host.example/path/to/somewhere", false)]
    /// Correct scheme to connect to localhost, with port 12345
    #[case::grpc_valid_ipv6_localhost_port_12345("grpc+http://[::1]:12345", true)]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[case::grpc_valid_http_host_without_port("grpc+http://localhost", true)]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[case::grpc_valid_https_host_without_port("grpc+https://localhost", true)]
    /// Correct scheme to connect to localhost over http, but with additional path, which is invalid.
    #[case::grpc_invalid_host_and_path("grpc+http://localhost/some-path", false)]
    /// A valid example for Bigtable
    #[cfg_attr(
        all(feature = "cloud", feature = "integration"),
        case::bigtable_valid_url(
            "bigtable://instance-1?project_id=project-1&table_name=table-1&family_name=cf1",
            true
        )
    )]
    /// A valid example for Bigtable, specifying a custom channel size and timeout
    #[cfg_attr(
        all(feature = "cloud", feature = "integration"),
        case::bigtable_valid_url(
            "bigtable://instance-1?project_id=project-1&table_name=table-1&family_name=cf1&channel_size=10&timeout=10",
            true
        )
    )]
    /// A invalid Bigtable example (missing fields)
    #[cfg_attr(
        all(feature = "cloud", feature = "integration"),
        case::bigtable_invalid_url("bigtable://instance-1", false)
    )]
    #[tokio::test]
    async fn test_from_addr_tokio(#[case] uri_str: &str, #[case] exp_succeed: bool) {
        if exp_succeed {
            from_addr(uri_str).await.expect("should succeed");
        } else {
            assert!(from_addr(uri_str).await.is_err(), "should fail");
        }
    }
}
