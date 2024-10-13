use std::sync::Arc;

use url::Url;

use crate::composition::{
    with_registry, CompositionContext, DeserializeWithRegistry, ServiceBuilder, REG,
};

use super::DirectoryService;

/// Constructs a new instance of a [DirectoryService] from an URI.
///
/// The following URIs are supported:
/// - `memory:`
///   Uses a in-memory implementation.
///   from one process at the same time.
/// - `redb:`
///   Uses a in-memory redb implementation.
/// - `redb:///absolute/path/to/somewhere`
///   Uses redb, using a path on the disk for persistency. Can be only opened
///   from one process at the same time.
/// - `grpc+unix:///absolute/path/to/somewhere`
///   Connects to a local tvix-store gRPC service via Unix socket.
/// - `grpc+http://host:port`, `grpc+https://host:port`
///    Connects to a (remote) tvix-store gRPC service.
pub async fn from_addr(
    uri: &str,
) -> Result<Arc<dyn DirectoryService>, Box<dyn std::error::Error + Send + Sync>> {
    #[allow(unused_mut)]
    let mut url = Url::parse(uri)
        .map_err(|e| crate::Error::StorageError(format!("unable to parse url: {}", e)))?;

    let directory_service_config = with_registry(&REG, || {
        <DeserializeWithRegistry<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>>>::try_from(
            url,
        )
    })?
    .0;
    let directory_service = directory_service_config
        .build("anonymous", &CompositionContext::blank(&REG))
        .await?;

    Ok(directory_service)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::from_addr;
    use rstest::rstest;
    use tempfile::TempDir;

    static TMPDIR_REDB_1: LazyLock<TempDir> = LazyLock::new(|| TempDir::new().unwrap());
    static TMPDIR_REDB_2: LazyLock<TempDir> = LazyLock::new(|| TempDir::new().unwrap());

    #[rstest]
    /// This uses an unsupported scheme.
    #[case::unsupported_scheme("http://foo.example/test", false)]
    /// This correctly sets the scheme, and doesn't set a path.
    #[case::memory_valid("memory://", true)]
    /// This sets a memory url host to `foo`
    #[case::memory_invalid_host("memory://foo", false)]
    /// This sets a memory url path to "/", which is invalid.
    #[case::memory_invalid_root_path("memory:///", false)]
    /// This sets a memory url path to "/foo", which is invalid.
    #[case::memory_invalid_root_path_foo("memory:///foo", false)]
    /// This configures redb in temporary mode.
    #[case::redb_valid_temporary("redb://", true)]
    /// This configures redb with /, which should fail.
    #[case::redb_invalid_root("redb:///", false)]
    /// This configures redb with a host, not path, which should fail.
    #[case::redb_invalid_host("redb://foo.example", false)]
    /// This configures redb with a valid path, which should succeed.
    #[case::redb_valid_path(&format!("redb://{}", &TMPDIR_REDB_1.path().join("foo").to_str().unwrap()), true)]
    /// This configures redb with a host, and a valid path path, which should fail.
    #[case::redb_invalid_host_with_valid_path(&format!("redb://foo.example{}", &TMPDIR_REDB_2.path().join("bar").to_str().unwrap()), false)]
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
    /// A valid example for store composition using anonymous urls
    #[cfg_attr(
        feature = "xp-store-composition",
        case::anonymous_url_composition("cache://?near=memory://&far=memory://", true)
    )]
    /// Store composition with anonymous urls should fail if the feature is disabled
    #[cfg_attr(
        not(feature = "xp-store-composition"),
        case::anonymous_url_composition("cache://?near=memory://&far=memory://", false)
    )]
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
