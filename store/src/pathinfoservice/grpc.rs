use super::PathInfoService;
use crate::proto;
use tonic::{transport::Channel, Code, Status};

/// Connects to a (remote) tvix-store PathInfoService over gRPC.
#[derive(Clone)]
pub struct GRPCPathInfoService {
    /// A handle into the active tokio runtime. Necessary to spawn tasks.
    tokio_handle: tokio::runtime::Handle,

    /// The internal reference to a gRPC client.
    /// Cloning it is cheap, and it internally handles concurrent requests.
    grpc_client: proto::path_info_service_client::PathInfoServiceClient<Channel>,
}

impl GRPCPathInfoService {
    /// Construct a new [GRPCPathInfoService], by passing a handle to the tokio
    /// runtime, and a gRPC client.
    pub fn new(
        tokio_handle: tokio::runtime::Handle,
        grpc_client: proto::path_info_service_client::PathInfoServiceClient<Channel>,
    ) -> Self {
        Self {
            tokio_handle,
            grpc_client,
        }
    }
}

impl PathInfoService for GRPCPathInfoService {
    fn get(
        &self,
        by_what: proto::get_path_info_request::ByWhat,
    ) -> Result<Option<proto::PathInfo>, crate::Error> {
        // Get a new handle to the gRPC client.
        let mut grpc_client = self.grpc_client.clone();

        let task: tokio::task::JoinHandle<Result<proto::PathInfo, Status>> =
            self.tokio_handle.spawn(async move {
                let path_info = grpc_client
                    .get(proto::GetPathInfoRequest {
                        by_what: Some(by_what),
                    })
                    .await?
                    .into_inner();

                Ok(path_info)
            });

        match self.tokio_handle.block_on(task)? {
            Ok(path_info) => Ok(Some(path_info)),
            Err(e) if e.code() == Code::NotFound => Ok(None),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    fn put(&self, path_info: proto::PathInfo) -> Result<proto::PathInfo, crate::Error> {
        // Get a new handle to the gRPC client.
        let mut grpc_client = self.grpc_client.clone();

        let task: tokio::task::JoinHandle<Result<proto::PathInfo, Status>> =
            self.tokio_handle.spawn(async move {
                let path_info = grpc_client.put(path_info).await?.into_inner();
                Ok(path_info)
            });

        self.tokio_handle
            .block_on(task)?
            .map_err(|e| crate::Error::StorageError(e.to_string()))
    }
}
