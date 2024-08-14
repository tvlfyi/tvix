use crate::directoryservice::{DirectoryGraph, DirectoryService, LeavesToRootValidator};
use crate::{proto, B3Digest, DirectoryError};
use futures::stream::BoxStream;
use futures::TryStreamExt;
use std::ops::Deref;
use tokio_stream::once;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::{instrument, warn};

pub struct GRPCDirectoryServiceWrapper<T> {
    directory_service: T,
}

impl<T> GRPCDirectoryServiceWrapper<T> {
    pub fn new(directory_service: T) -> Self {
        Self { directory_service }
    }
}

#[async_trait]
impl<T> proto::directory_service_server::DirectoryService for GRPCDirectoryServiceWrapper<T>
where
    T: Deref<Target = dyn DirectoryService> + Send + Sync + 'static,
{
    type GetStream = BoxStream<'static, tonic::Result<proto::Directory, Status>>;

    #[instrument(skip_all)]
    async fn get<'a>(
        &'a self,
        request: Request<proto::GetDirectoryRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let req_inner = request.into_inner();

        let by_what = &req_inner
            .by_what
            .ok_or_else(|| Status::invalid_argument("invalid by_what"))?;

        match by_what {
            proto::get_directory_request::ByWhat::Digest(ref digest) => {
                let digest: B3Digest = digest
                    .clone()
                    .try_into()
                    .map_err(|_e| Status::invalid_argument("invalid digest length"))?;

                Ok(tonic::Response::new({
                    if !req_inner.recursive {
                        let directory = self
                            .directory_service
                            .get(&digest)
                            .await
                            .map_err(|e| {
                                warn!(err = %e, directory.digest=%digest, "failed to get directory");
                                tonic::Status::new(tonic::Code::Internal, e.to_string())
                            })?
                            .ok_or_else(|| {
                                Status::not_found(format!("directory {} not found", digest))
                            })?;

                        Box::pin(once(Ok(directory.into())))
                    } else {
                        // If recursive was requested, traverse via get_recursive.
                        Box::pin(
                            self.directory_service
                                .get_recursive(&digest)
                                .map_ok(proto::Directory::from)
                                .map_err(|e| {
                                    tonic::Status::new(tonic::Code::Internal, e.to_string())
                                }),
                        )
                    }
                }))
            }
        }
    }

    #[instrument(skip_all)]
    async fn put(
        &self,
        request: Request<Streaming<proto::Directory>>,
    ) -> Result<Response<proto::PutDirectoryResponse>, Status> {
        let mut req_inner = request.into_inner();

        // We put all Directory messages we receive into DirectoryGraph.
        let mut validator = DirectoryGraph::<LeavesToRootValidator>::default();
        while let Some(directory) = req_inner.message().await? {
            validator
                .add(directory.try_into().map_err(|e: DirectoryError| {
                    tonic::Status::new(tonic::Code::Internal, e.to_string())
                })?)
                .map_err(|e| tonic::Status::new(tonic::Code::Internal, e.to_string()))?;
        }

        // drain, which validates connectivity too.
        let directories = validator
            .validate()
            .map_err(|e| tonic::Status::new(tonic::Code::Internal, e.to_string()))?
            .drain_leaves_to_root()
            .collect::<Vec<_>>();

        let mut directory_putter = self.directory_service.put_multiple_start();
        for directory in directories {
            directory_putter.put(directory).await?;
        }

        // Properly close the directory putter. Peek at last_directory_digest
        // and return it, or propagate errors.
        let last_directory_dgst = directory_putter.close().await?;

        Ok(Response::new(proto::PutDirectoryResponse {
            root_digest: last_directory_dgst.into(),
        }))
    }
}
