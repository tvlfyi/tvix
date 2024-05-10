use super::PathInfoService;
use crate::proto::{self, ListPathInfoRequest, PathInfo};
use async_stream::try_stream;
use data_encoding::BASE64;
use futures::stream::BoxStream;
use tonic::{async_trait, transport::Channel, Code};
use tracing::instrument;
use tvix_castore::{proto as castorepb, Error};

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
    #[instrument(level = "trace", skip_all, fields(path_info.digest = BASE64.encode(&digest)))]
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
            Ok(path_info) => {
                let path_info = path_info.into_inner();

                path_info
                    .validate()
                    .map_err(|e| Error::StorageError(format!("invalid pathinfo: {}", e)))?;

                Ok(Some(path_info))
            }
            Err(e) if e.code() == Code::NotFound => Ok(None),
            Err(e) => Err(Error::StorageError(e.to_string())),
        }
    }

    #[instrument(level = "trace", skip_all, fields(path_info.root_node = ?path_info.node))]
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

    #[instrument(level = "trace", skip_all, fields(root_node = ?root_node))]
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

    #[instrument(level = "trace", skip_all)]
    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
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
    use crate::pathinfoservice::tests::make_grpc_path_info_service_client;
    use crate::tests::fixtures;

    /// This ensures connecting via gRPC works as expected.
    #[tokio::test]
    async fn test_valid_unix_path_ping_pong() {
        let (_blob_service, _directory_service, path_info_service) =
            make_grpc_path_info_service_client().await;

        let path_info = path_info_service
            .get(fixtures::DUMMY_PATH_DIGEST)
            .await
            .expect("must not be error");

        assert!(path_info.is_none());
    }
}
