use super::{grpc::GRPCBuildService, BuildService, DummyBuildService};
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService};
use url::Url;

/// Constructs a new instance of a [BuildService] from an URI.
///
/// The following schemes are supported by the following services:
/// - `dummy://` ([DummyBuildService])
/// - `grpc+*://` ([GRPCBuildService])
///
/// As some of these [BuildService] need to talk to a [BlobService] and
/// [DirectoryService], these also need to be passed in.
pub async fn from_addr<BS, DS>(
    uri: &str,
    _blob_service: BS,
    _directory_service: DS,
) -> std::io::Result<Box<dyn BuildService>>
where
    BS: AsRef<dyn BlobService> + Send + Sync + Clone + 'static,
    DS: AsRef<dyn DirectoryService> + Send + Sync + Clone + 'static,
{
    let url = Url::parse(uri)
        .map_err(|e| std::io::Error::other(format!("unable to parse url: {}", e)))?;

    Ok(match url.scheme() {
        // dummy doesn't care about parameters.
        "dummy" => Box::<DummyBuildService>::default(),
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
    use std::sync::Arc;

    use super::from_addr;
    use test_case::test_case;
    use tvix_castore::{
        blobservice::{BlobService, MemoryBlobService},
        directoryservice::{DirectoryService, MemoryDirectoryService},
    };

    /// This uses an unsupported scheme.
    #[test_case("http://foo.example/test", false; "unsupported scheme")]
    /// This configures dummy
    #[test_case("dummy://", true; "valid dummy")]
    /// Correct scheme to connect to a unix socket.
    #[test_case("grpc+unix:///path/to/somewhere", true; "grpc valid unix socket")]
    /// Correct scheme for unix socket, but setting a host too, which is invalid.
    #[test_case("grpc+unix://host.example/path/to/somewhere", false; "grpc invalid unix socket and host")]
    /// Correct scheme to connect to localhost, with port 12345
    #[test_case("grpc+http://[::1]:12345", true; "grpc valid IPv6 localhost port 12345")]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[test_case("grpc+http://localhost", true; "grpc valid http host without port")]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[test_case("grpc+https://localhost", true; "grpc valid https host without port")]
    /// Correct scheme to connect to localhost over http, but with additional path, which is invalid.
    #[test_case("grpc+http://localhost/some-path", false; "grpc valid invalid host and path")]
    #[tokio::test]
    async fn test_from_addr(uri_str: &str, is_ok: bool) {
        let blob_service: Arc<dyn BlobService> = Arc::from(MemoryBlobService::default());
        let directory_service: Arc<dyn DirectoryService> =
            Arc::from(MemoryDirectoryService::default());
        assert_eq!(
            from_addr(uri_str, blob_service, directory_service)
                .await
                .is_ok(),
            is_ok
        )
    }
}
