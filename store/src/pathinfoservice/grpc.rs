use super::PathInfoService;
use crate::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    proto::{self, ListPathInfoRequest},
};
use async_stream::try_stream;
use futures::Stream;
use std::{pin::Pin, sync::Arc};
use tokio::net::UnixStream;
use tonic::{async_trait, transport::Channel, Code};

/// Connects to a (remote) tvix-store PathInfoService over gRPC.
#[derive(Clone)]
pub struct GRPCPathInfoService {
    /// The internal reference to a gRPC client.
    /// Cloning it is cheap, and it internally handles concurrent requests.
    grpc_client: proto::path_info_service_client::PathInfoServiceClient<Channel>,
}

impl GRPCPathInfoService {
    /// construct a [GRPCPathInfoService] from a [proto::path_info_service_client::PathInfoServiceClient].
    /// panics if called outside the context of a tokio runtime.
    pub fn from_client(
        grpc_client: proto::path_info_service_client::PathInfoServiceClient<Channel>,
    ) -> Self {
        Self { grpc_client }
    }
}

#[async_trait]
impl PathInfoService for GRPCPathInfoService {
    /// Constructs a [GRPCPathInfoService] from the passed [url::Url]:
    /// - scheme has to match `grpc+*://`.
    ///   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
    /// - In the case of unix sockets, there must be a path, but may not be a host.
    /// - In the case of non-unix sockets, there must be a host, but no path.
    /// The blob_service and directory_service arguments are ignored, because the gRPC service already provides answers to these questions.
    fn from_url(
        url: &url::Url,
        _blob_service: Arc<dyn BlobService>,
        _directory_service: Arc<dyn DirectoryService>,
    ) -> Result<Self, crate::Error> {
        // Start checking for the scheme to start with grpc+.
        match url.scheme().strip_prefix("grpc+") {
            None => Err(crate::Error::StorageError("invalid scheme".to_string())),
            Some(rest) => {
                if rest == "unix" {
                    if url.host_str().is_some() {
                        return Err(crate::Error::StorageError(
                            "host may not be set".to_string(),
                        ));
                    }
                    let path = url.path().to_string();
                    let channel = tonic::transport::Endpoint::try_from("http://[::]:50051") // doesn't matter
                        .unwrap()
                        .connect_with_connector_lazy(tower::service_fn(
                            move |_: tonic::transport::Uri| UnixStream::connect(path.clone()),
                        ));
                    let grpc_client =
                        proto::path_info_service_client::PathInfoServiceClient::new(channel);
                    Ok(Self::from_client(grpc_client))
                } else {
                    // ensure path is empty, not supported with gRPC.
                    if !url.path().is_empty() {
                        return Err(crate::Error::StorageError(
                            "path may not be set".to_string(),
                        ));
                    }

                    // clone the uri, and drop the grpc+ from the scheme.
                    // Recreate a new uri with the `grpc+` prefix dropped from the scheme.
                    // We can't use `url.set_scheme(rest)`, as it disallows
                    // setting something http(s) that previously wasn't.
                    let url = {
                        let url_str = url.to_string();
                        let s_stripped = url_str.strip_prefix("grpc+").unwrap();
                        url::Url::parse(s_stripped).unwrap()
                    };
                    let channel = tonic::transport::Endpoint::try_from(url.to_string())
                        .unwrap()
                        .connect_lazy();

                    let grpc_client =
                        proto::path_info_service_client::PathInfoServiceClient::new(channel);
                    Ok(Self::from_client(grpc_client))
                }
            }
        }
    }

    async fn get(&self, digest: [u8; 20]) -> Result<Option<proto::PathInfo>, crate::Error> {
        // Get a new handle to the gRPC client.
        let mut grpc_client = self.grpc_client.clone();

        let path_info = grpc_client
            .get(proto::GetPathInfoRequest {
                by_what: Some(proto::get_path_info_request::ByWhat::ByOutputHash(
                    digest.to_vec().into(),
                )),
            })
            .await;

        match path_info {
            Ok(path_info) => Ok(Some(path_info.into_inner())),
            Err(e) if e.code() == Code::NotFound => Ok(None),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    async fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, crate::Error> {
        // Get a new handle to the gRPC client.
        let mut grpc_client = self.grpc_client.clone();

        let path_info = grpc_client
            .put(path_info)
            .await
            .map_err(|e| crate::Error::StorageError(e.to_string()))?
            .into_inner();

        Ok(path_info)
    }

    async fn calculate_nar(
        &self,
        root_node: &proto::node::Node,
    ) -> Result<(u64, [u8; 32]), crate::Error> {
        // Get a new handle to the gRPC client.
        let mut grpc_client = self.grpc_client.clone();
        let root_node = root_node.clone();

        let path_info = grpc_client
            .calculate_nar(proto::Node {
                node: Some(root_node),
            })
            .await
            .map_err(|e| crate::Error::StorageError(e.to_string()))?
            .into_inner();

        let nar_sha256: [u8; 32] = path_info
            .nar_sha256
            .to_vec()
            .try_into()
            .map_err(|_e| crate::Error::StorageError("invalid digest length".to_string()))?;

        Ok((path_info.nar_size, nar_sha256))
    }

    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<proto::PathInfo, crate::Error>> + Send>> {
        let mut grpc_client = self.grpc_client.clone();

        let stream = try_stream! {
            let resp = grpc_client.list(ListPathInfoRequest::default()).await;

            let mut stream = resp.map_err(|e| crate::Error::StorageError(e.to_string()))?.into_inner();

            loop {
                match stream.message().await {
                    Ok(o) => match o {
                        Some(pathinfo) => {
                            // validate the pathinfo
                            if let Err(e) = pathinfo.validate() {
                                Err(crate::Error::StorageError(format!(
                                    "pathinfo {:?} failed validation: {}",
                                    pathinfo, e
                                )))?;
                            }
                            yield pathinfo
                        }
                        None => {
                            return;
                        },
                    },
                    Err(e) => Err(crate::Error::StorageError(e.to_string()))?,
                }
            }
        };

        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use tempfile::TempDir;
    use tokio::net::UnixListener;
    use tokio::time;
    use tokio_stream::wrappers::UnixListenerStream;

    use crate::pathinfoservice::MemoryPathInfoService;
    use crate::proto::GRPCPathInfoServiceWrapper;
    use crate::tests::fixtures;
    use crate::tests::utils::gen_blob_service;
    use crate::tests::utils::gen_directory_service;

    use super::GRPCPathInfoService;
    use super::PathInfoService;

    /// This uses the wrong scheme
    #[test]
    fn test_invalid_scheme() {
        let url = url::Url::parse("http://foo.example/test").expect("must parse");

        assert!(
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }

    /// This uses the correct scheme for a unix socket.
    /// The fact that /path/to/somewhere doesn't exist yet is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_valid_unix_path() {
        let url = url::Url::parse("grpc+unix:///path/to/somewhere").expect("must parse");

        assert!(
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_ok()
        );
    }

    /// This uses the correct scheme for a unix socket,
    /// but sets a host, which is unsupported.
    #[tokio::test]
    async fn test_invalid_unix_path_with_domain() {
        let url =
            url::Url::parse("grpc+unix://host.example/path/to/somewhere").expect("must parse");

        assert!(
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }

    /// This uses the correct scheme for a HTTP server.
    /// The fact that nothing is listening there is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_valid_http() {
        let url = url::Url::parse("grpc+http://localhost").expect("must parse");

        assert!(
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_ok()
        );
    }

    /// This uses the correct scheme for a HTTPS server.
    /// The fact that nothing is listening there is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_valid_https() {
        let url = url::Url::parse("grpc+https://localhost").expect("must parse");

        assert!(
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_ok()
        );
    }

    /// This uses the correct scheme, but also specifies
    /// an additional path, which is not supported for gRPC.
    /// The fact that nothing is listening there is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_invalid_http_with_path() {
        let url = url::Url::parse("grpc+https://localhost/some-path").expect("must parse");

        assert!(
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .is_err()
        );
    }

    /// This uses the correct scheme for a unix socket, and provides a server on the other side.
    #[tokio::test]
    async fn test_valid_unix_path_ping_pong() {
        let tmpdir = TempDir::new().unwrap();
        let path = tmpdir.path().join("daemon");

        // let mut join_set = JoinSet::new();

        // prepare a client
        let client = {
            let mut url = url::Url::parse("grpc+unix:///path/to/somewhere").expect("must parse");
            url.set_path(path.to_str().unwrap());
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .expect("must succeed")
        };

        let path_copy = path.clone();

        // Spin up a server, in a thread far away, which spawns its own tokio runtime,
        // and blocks on the task.
        thread::spawn(move || {
            // Create the runtime
            let rt = tokio::runtime::Runtime::new().unwrap();
            // Get a handle from this runtime
            let handle = rt.handle();

            let task = handle.spawn(async {
                let uds = UnixListener::bind(path_copy).unwrap();
                let uds_stream = UnixListenerStream::new(uds);

                // spin up a new server
                let mut server = tonic::transport::Server::builder();
                let router = server.add_service(
                    crate::proto::path_info_service_server::PathInfoServiceServer::new(
                        GRPCPathInfoServiceWrapper::from(Arc::new(MemoryPathInfoService::new(
                            gen_blob_service(),
                            gen_directory_service(),
                        ))
                            as Arc<dyn PathInfoService>),
                    ),
                );
                router.serve_with_incoming(uds_stream).await
            });

            handle.block_on(task)
        });

        // wait for the socket to be created
        {
            let mut socket_created = false;
            for _try in 1..20 {
                if path.exists() {
                    socket_created = true;
                    break;
                }
                tokio::time::sleep(time::Duration::from_millis(20)).await;
            }

            assert!(
                socket_created,
                "expected socket path to eventually get created, but never happened"
            );
        }

        let pi = client
            .get(fixtures::DUMMY_OUTPUT_HASH.to_vec().try_into().unwrap())
            .await
            .expect("must not be error");

        assert!(pi.is_none());
    }
}
