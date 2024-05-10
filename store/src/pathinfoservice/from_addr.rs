use crate::proto::path_info_service_client::PathInfoServiceClient;

use super::{
    GRPCPathInfoService, MemoryPathInfoService, NixHTTPPathInfoService, PathInfoService,
    SledPathInfoService,
};

use nix_compat::narinfo;
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
/// - `nix+https://cache.nixos.org?trusted-public-keys=cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=`
///   Exposes the Nix binary cache as a PathInfoService, ingesting NARs into the
///   {Blob,Directory}Service. You almost certainly want to use this with some cache.
///   The `trusted-public-keys` URL parameter can be provided, which will then
///   enable signature verification.
/// - `grpc+unix:///absolute/path/to/somewhere`
///   Connects to a local tvix-store gRPC service via Unix socket.
/// - `grpc+http://host:port`, `grpc+https://host:port`
///    Connects to a (remote) tvix-store gRPC service.
///
/// As the [PathInfoService] needs to talk to [BlobService] and [DirectoryService],
/// these also need to be passed in.
pub async fn from_addr(
    uri: &str,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<Box<dyn PathInfoService>, Error> {
    #[allow(unused_mut)]
    let mut url =
        Url::parse(uri).map_err(|e| Error::StorageError(format!("unable to parse url: {}", e)))?;

    let path_info_service: Box<dyn PathInfoService> = match url.scheme() {
        "memory" => {
            // memory doesn't support host or path in the URL.
            if url.has_host() || !url.path().is_empty() {
                return Err(Error::StorageError("invalid url".to_string()));
            }
            Box::<MemoryPathInfoService>::default()
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

            // TODO: expose other parameters as URL parameters?

            Box::new(if url.path().is_empty() {
                SledPathInfoService::new_temporary()
                    .map_err(|e| Error::StorageError(e.to_string()))?
            } else {
                SledPathInfoService::new(url.path())
                    .map_err(|e| Error::StorageError(e.to_string()))?
            })
        }
        "nix+http" | "nix+https" => {
            // Stringify the URL and remove the nix+ prefix.
            // We can't use `url.set_scheme(rest)`, as it disallows
            // setting something http(s) that previously wasn't.
            let new_url = Url::parse(url.to_string().strip_prefix("nix+").unwrap()).unwrap();

            let mut nix_http_path_info_service =
                NixHTTPPathInfoService::new(new_url, blob_service, directory_service);

            let pairs = &url.query_pairs();
            for (k, v) in pairs.into_iter() {
                if k == "trusted-public-keys" {
                    let pubkey_strs: Vec<_> = v.split_ascii_whitespace().collect();

                    let mut pubkeys: Vec<narinfo::PubKey> = Vec::with_capacity(pubkey_strs.len());
                    for pubkey_str in pubkey_strs {
                        pubkeys.push(narinfo::PubKey::parse(pubkey_str).map_err(|e| {
                            Error::StorageError(format!("invalid public key: {e}"))
                        })?);
                    }

                    nix_http_path_info_service.set_public_keys(pubkeys);
                }
            }

            Box::new(nix_http_path_info_service)
        }
        scheme if scheme.starts_with("grpc+") => {
            // schemes starting with grpc+ go to the GRPCPathInfoService.
            //   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
            // - In the case of unix sockets, there must be a path, but may not be a host.
            // - In the case of non-unix sockets, there must be a host, but no path.
            // Constructing the channel is handled by tvix_castore::channel::from_url.
            let client =
                PathInfoServiceClient::new(tvix_castore::tonic::channel_from_url(&url).await?);
            Box::new(GRPCPathInfoService::from_client(client))
        }
        #[cfg(feature = "cloud")]
        "bigtable" => {
            use super::bigtable::BigtableParameters;
            use super::BigtablePathInfoService;

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
                BigtablePathInfoService::connect(params)
                    .await
                    .map_err(|e| Error::StorageError(e.to_string()))?,
            )
        }
        _ => Err(Error::StorageError(format!(
            "unknown scheme: {}",
            url.scheme()
        )))?,
    };

    Ok(path_info_service)
}

#[cfg(test)]
mod tests {
    use super::from_addr;
    use lazy_static::lazy_static;
    use rstest::rstest;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tvix_castore::{
        blobservice::{BlobService, MemoryBlobService},
        directoryservice::{DirectoryService, MemoryDirectoryService},
    };

    lazy_static! {
        static ref TMPDIR_SLED_1: TempDir = TempDir::new().unwrap();
        static ref TMPDIR_SLED_2: TempDir = TempDir::new().unwrap();
    }

    // the gRPC tests below don't fail, because we connect lazily.

    #[rstest]
    /// This uses a unsupported scheme.
    #[case::unsupported_scheme("http://foo.example/test", false)]
    /// This configures sled in temporary mode.
    #[case::sled_temporary("sled://", true)]
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
    /// Correct Scheme for the cache.nixos.org binary cache.
    #[case::correct_nix_https("nix+https://cache.nixos.org", true)]
    /// Correct Scheme for the cache.nixos.org binary cache (HTTP URL).
    #[case::correct_nix_http("nix+http://cache.nixos.org", true)]
    /// Correct Scheme for Nix HTTP Binary cache, with a subpath.
    #[case::correct_nix_http_with_subpath("nix+http://192.0.2.1/foo", true)]
    /// Correct Scheme for Nix HTTP Binary cache, with a subpath and port.
    #[case::correct_nix_http_with_subpath_and_port("nix+http://[::1]:8080/foo", true)]
    /// Correct Scheme for the cache.nixos.org binary cache, and correct trusted public key set
    #[case::correct_nix_https_with_trusted_public_key("nix+https://cache.nixos.org?trusted-public-keys=cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=", true)]
    /// Correct Scheme for the cache.nixos.org binary cache, and two correct trusted public keys set
    #[case::correct_nix_https_with_two_trusted_public_keys("nix+https://cache.nixos.org?trusted-public-keys=cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=%20foo:jp4fCEx9tBEId/L0ZsVJ26k0wC0fu7vJqLjjIGFkup8=", true)]
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
    /// A valid example for Bigtable.
    #[cfg_attr(
        all(feature = "cloud", feature = "integration"),
        case::bigtable_valid(
            "bigtable://instance-1?project_id=project-1&table_name=table-1&family_name=cf1",
            true
        )
    )]
    /// An invalid example for Bigtable, missing fields
    #[cfg_attr(
        all(feature = "cloud", feature = "integration"),
        case::bigtable_invalid_missing_fields("bigtable://instance-1", false)
    )]
    #[tokio::test]
    async fn test_from_addr_tokio(#[case] uri_str: &str, #[case] exp_succeed: bool) {
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
