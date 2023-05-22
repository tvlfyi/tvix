use super::NARCalculationService;
use crate::proto;
use tonic::transport::Channel;
use tonic::Status;

/// A NAR calculation service which asks a remote tvix-store for NAR calculation
/// (via the gRPC PathInfoService).
#[derive(Clone)]
pub struct GRPCNARCalculationService {
    /// A handle into the active tokio runtime. Necessary to spawn tasks.
    tokio_handle: tokio::runtime::Handle,

    /// The internal reference to a gRPC client.
    /// Cloning it is cheap, and it internally handles concurrent requests.
    grpc_client: proto::path_info_service_client::PathInfoServiceClient<Channel>,
}

impl GRPCNARCalculationService {
    /// construct a new [GRPCNARCalculationService], by passing a handle to the
    /// tokio runtime, and a gRPC client.
    pub fn new(
        tokio_handle: tokio::runtime::Handle,
        grpc_client: proto::path_info_service_client::PathInfoServiceClient<Channel>,
    ) -> Self {
        Self {
            tokio_handle,
            grpc_client,
        }
    }

    /// construct a [GRPCNARCalculationService], from a [proto::path_info_service_client::PathInfoServiceClient<Channel>].
    /// panics if called outside the context of a tokio runtime.
    pub fn from_client(
        grpc_client: proto::path_info_service_client::PathInfoServiceClient<Channel>,
    ) -> Self {
        Self {
            tokio_handle: tokio::runtime::Handle::current(),
            grpc_client,
        }
    }
}

impl NARCalculationService for GRPCNARCalculationService {
    fn calculate_nar(
        &self,
        root_node: &proto::node::Node,
    ) -> Result<(u64, [u8; 32]), super::RenderError> {
        // Get a new handle to the gRPC client, and copy the root node.
        let mut grpc_client = self.grpc_client.clone();
        let root_node = root_node.clone();

        let task: tokio::task::JoinHandle<Result<_, Status>> =
            self.tokio_handle.spawn(async move {
                Ok(grpc_client
                    .calculate_nar(proto::Node {
                        node: Some(root_node),
                    })
                    .await?
                    .into_inner())
            });

        match self.tokio_handle.block_on(task).unwrap() {
            Ok(resp) => Ok((resp.nar_size, resp.nar_sha256.to_vec().try_into().unwrap())),
            Err(e) => Err(super::RenderError::StoreError(crate::Error::StorageError(
                e.to_string(),
            ))),
        }
    }
}
