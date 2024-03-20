use crate::directoryservice::ClosureValidator;
use crate::proto;
use crate::{directoryservice::DirectoryService, B3Digest};
use futures::StreamExt;
use std::ops::Deref;
use tokio::sync::mpsc::channel;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::{debug, instrument, warn};

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
    type GetStream = ReceiverStream<tonic::Result<proto::Directory, Status>>;

    #[instrument(skip_all)]
    async fn get(
        &self,
        request: Request<proto::GetDirectoryRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let (tx, rx) = channel(5);

        let req_inner = request.into_inner();

        // look at the digest in the request and put it in the top of the queue.
        match &req_inner.by_what {
            None => return Err(Status::invalid_argument("by_what needs to be specified")),
            Some(proto::get_directory_request::ByWhat::Digest(ref digest)) => {
                let digest: B3Digest = digest
                    .clone()
                    .try_into()
                    .map_err(|_e| Status::invalid_argument("invalid digest length"))?;

                if !req_inner.recursive {
                    let e: Result<proto::Directory, Status> = match self
                        .directory_service
                        .get(&digest)
                        .await
                    {
                        Ok(Some(directory)) => Ok(directory),
                        Ok(None) => {
                            Err(Status::not_found(format!("directory {} not found", digest)))
                        }
                        Err(e) => {
                            warn!(err = %e, directory.digest=%digest, "failed to get directory");
                            Err(e.into())
                        }
                    };

                    if tx.send(e).await.is_err() {
                        debug!("receiver dropped");
                    }
                } else {
                    // If recursive was requested, traverse via get_recursive.
                    let mut directories_it = self.directory_service.get_recursive(&digest);

                    while let Some(e) = directories_it.next().await {
                        // map err in res from Error to Status
                        let res = e.map_err(|e| Status::internal(e.to_string()));
                        if tx.send(res).await.is_err() {
                            debug!("receiver dropped");
                            break;
                        }
                    }
                }
            }
        }

        let receiver_stream = ReceiverStream::new(rx);
        Ok(Response::new(receiver_stream))
    }

    #[instrument(skip_all)]
    async fn put(
        &self,
        request: Request<Streaming<proto::Directory>>,
    ) -> Result<Response<proto::PutDirectoryResponse>, Status> {
        let mut req_inner = request.into_inner();

        // We put all Directory messages we receive into ClosureValidator first.
        let mut validator = ClosureValidator::default();
        while let Some(directory) = req_inner.message().await? {
            validator.add(directory)?;
        }

        // drain, which validates connectivity too.
        let directories = validator.finalize()?;

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
