use super::PathInfoService;
use crate::proto::{self, ListPathInfoRequest, PathInfo};
use async_stream::try_stream;
use futures::Stream;
use std::{pin::Pin, sync::Arc};
use tonic::{async_trait, transport::Channel, Code};
use tvix_castore::{
    blobservice::BlobService, directoryservice::DirectoryService, proto as castorepb, Error,
};

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

    /// Constructs a [GRPCPathInfoService] from the passed [url::Url]:
    /// - scheme has to match `grpc+*://`.
    ///   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
    /// - In the case of unix sockets, there must be a path, but may not be a host.
    /// - In the case of non-unix sockets, there must be a host, but no path.
    /// The blob_service and directory_service arguments are ignored, because the gRPC service already provides answers to these questions.
    pub fn from_url(
        url: &url::Url,
        _blob_service: Arc<dyn BlobService>,
        _directory_service: Arc<dyn DirectoryService>,
    ) -> Result<Self, tvix_castore::Error> {
        let channel = tvix_castore::channel::from_url(url)?;
        Ok(Self::from_client(
            proto::path_info_service_client::PathInfoServiceClient::new(channel),
        ))
    }
}

#[async_trait]
impl PathInfoService for GRPCPathInfoService {
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        let path_info = self
            .grpc_client
            .clone()
            .get(proto::GetPathInfoRequest {
                by_what: Some(proto::get_path_info_request::ByWhat::ByOutputHash(
                    digest.to_vec().into(),
                )),
            })
            .await;

        match path_info {
            Ok(path_info) => Ok(Some(path_info.into_inner())),
            Err(e) if e.code() == Code::NotFound => Ok(None),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error> {
        let path_info = self
            .grpc_client
            .clone()
            .put(path_info)
            .await
            .map_err(|e| Error::StorageError(e.to_string()))?
            .into_inner();

        Ok(path_info)
    }

    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), Error> {
        let path_info = self
            .grpc_client
            .clone()
            .calculate_nar(castorepb::Node {
                node: Some(root_node.clone()),
            })
            .await
            .map_err(|e| Error::StorageError(e.to_string()))?
            .into_inner();

        let nar_sha256: [u8; 32] = path_info
            .nar_sha256
            .to_vec()
            .try_into()
            .map_err(|_e| Error::StorageError("invalid digest length".to_string()))?;

        Ok((path_info.nar_size, nar_sha256))
    }

    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<PathInfo, Error>> + Send>> {
        let mut grpc_client = self.grpc_client.clone();

        let stream = try_stream! {
            let resp = grpc_client.list(ListPathInfoRequest::default()).await;

            let mut stream = resp.map_err(|e| Error::StorageError(e.to_string()))?.into_inner();

            loop {
                match stream.message().await {
                    Ok(o) => match o {
                        Some(pathinfo) => {
                            // validate the pathinfo
                            if let Err(e) = pathinfo.validate() {
                                Err(Error::StorageError(format!(
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
                    Err(e) => Err(Error::StorageError(e.to_string()))?,
                }
            }
        };

        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tempfile::TempDir;
    use tokio::net::UnixListener;
    use tokio_retry::strategy::ExponentialBackoff;
    use tokio_retry::Retry;
    use tokio_stream::wrappers::UnixListenerStream;

    use crate::pathinfoservice::MemoryPathInfoService;
    use crate::proto::path_info_service_client::PathInfoServiceClient;
    use crate::proto::GRPCPathInfoServiceWrapper;
    use crate::tests::fixtures;
    use crate::tests::utils::gen_blob_service;
    use crate::tests::utils::gen_directory_service;

    use super::GRPCPathInfoService;
    use super::PathInfoService;

    /// This ensures connecting via gRPC works as expected.
    #[tokio::test]
    async fn test_valid_unix_path_ping_pong() {
        let tmpdir = TempDir::new().unwrap();
        let socket_path = tmpdir.path().join("daemon");

        let path_clone = socket_path.clone();

        // Spin up a server
        tokio::spawn(async {
            let uds = UnixListener::bind(path_clone).unwrap();
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

        // wait for the socket to be created
        Retry::spawn(
            ExponentialBackoff::from_millis(20).max_delay(Duration::from_secs(10)),
            || async {
                if socket_path.exists() {
                    Ok(())
                } else {
                    Err(())
                }
            },
        )
        .await
        .expect("failed to wait for socket");

        // prepare a client
        let grpc_client = {
            let url = url::Url::parse(&format!("grpc+unix://{}", socket_path.display()))
                .expect("must parse");
            GRPCPathInfoService::from_url(&url, gen_blob_service(), gen_directory_service())
                .expect("must succeed")
        };

        let path_info = grpc_client
            .get(fixtures::DUMMY_OUTPUT_HASH.to_vec().try_into().unwrap())
            .await
            .expect("must not be error");

        assert!(path_info.is_none());
    }
}
