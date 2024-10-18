use super::{grpc::GRPCBuildService, BuildService, DummyBuildService};
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService};
use url::Url;

#[cfg(target_os = "linux")]
use super::oci::OCIBuildService;

/// Constructs a new instance of a [BuildService] from an URI.
///
/// The following schemes are supported by the following services:
/// - `dummy://` ([DummyBuildService])
/// - `oci://` ([OCIBuildService])
/// - `grpc+*://` ([GRPCBuildService])
///
/// As some of these [BuildService] need to talk to a [BlobService] and
/// [DirectoryService], these also need to be passed in.
pub async fn from_addr<BS, DS>(
    uri: &str,
    blob_service: BS,
    directory_service: DS,
) -> std::io::Result<Box<dyn BuildService>>
where
    BS: BlobService + Send + Sync + Clone + 'static,
    DS: DirectoryService + Send + Sync + Clone + 'static,
{
    let url = Url::parse(uri)
        .map_err(|e| std::io::Error::other(format!("unable to parse url: {}", e)))?;

    Ok(match url.scheme() {
        // dummy doesn't care about parameters.
        "dummy" => Box::<DummyBuildService>::default(),
        #[cfg(target_os = "linux")]
        "oci" => {
            // oci wants a path in which it creates bundles.
            if url.path().is_empty() {
                Err(std::io::Error::other("oci needs a bundle dir as path"))?
            }

            // TODO: make sandbox shell and rootless_uid_gid

            Box::new(OCIBuildService::new(
                url.path().into(),
                blob_service,
                directory_service,
            ))
        }
        scheme => {
            if scheme.starts_with("grpc+") {
                let client = crate::proto::build_service_client::BuildServiceClient::new(
                    tvix_castore::tonic::channel_from_url(&url)
                        .await
                        .map_err(std::io::Error::other)?,
                );
                // FUTUREWORK: also allow responding to {blob,directory}_service
                // requests from the remote BuildService?
                Box::new(GRPCBuildService::from_client(client))
            } else {
                Err(std::io::Error::other(format!(
                    "unknown scheme: {}",
                    url.scheme()
                )))?
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::from_addr;
    use rstest::rstest;
    use std::sync::{Arc, LazyLock};
    use tempfile::TempDir;
    use tvix_castore::{
        blobservice::{BlobService, MemoryBlobService},
        directoryservice::{DirectoryService, MemoryDirectoryService},
    };

    static TMPDIR_OCI_1: LazyLock<TempDir> = LazyLock::new(|| TempDir::new().unwrap());

    #[rstest]
    /// This uses an unsupported scheme.
    #[case::unsupported_scheme("http://foo.example/test", false)]
    /// This configures dummy
    #[case::valid_dummy("dummy://", true)]
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
    /// This configures OCI, but doesn't specify the bundle path
    #[case::oci_missing_bundle_dir("oci://", false)]
    /// This configures OCI, specifying the bundle path
    #[case::oci_bundle_path(&format!("oci://{}", TMPDIR_OCI_1.path().to_str().unwrap()), true)]
    #[tokio::test]
    async fn test_from_addr(#[case] uri_str: &str, #[case] exp_succeed: bool) {
        let blob_service: Arc<dyn BlobService> = Arc::from(MemoryBlobService::default());
        let directory_service: Arc<dyn DirectoryService> =
            Arc::from(MemoryDirectoryService::default());

        let resp = from_addr(uri_str, blob_service, directory_service).await;

        if exp_succeed {
            resp.expect("should succeed");
        } else {
            assert!(resp.is_err(), "should fail");
        }
    }
}
