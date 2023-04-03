use crate::directoryservice::DirectoryService;
use crate::proto;
use data_encoding::BASE64;
use std::collections::HashMap;
use tokio::{sync::mpsc::channel, task};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::{debug, instrument, warn};

pub struct GRPCDirectoryServiceWrapper<C: DirectoryService> {
    directory_service: C,
}

impl<DS: DirectoryService> From<DS> for GRPCDirectoryServiceWrapper<DS> {
    fn from(value: DS) -> Self {
        Self {
            directory_service: value,
        }
    }
}

#[async_trait]
impl<DS: DirectoryService + Send + Sync + Clone + 'static>
    proto::directory_service_server::DirectoryService for GRPCDirectoryServiceWrapper<DS>
{
    type GetStream = ReceiverStream<tonic::Result<proto::Directory, Status>>;

    #[instrument(skip(self))]
    async fn get(
        &self,
        request: Request<proto::GetDirectoryRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let (tx, rx) = channel(5);

        let req_inner = request.into_inner();

        let directory_service = self.directory_service.clone();

        let _task = {
            // look at the digest in the request and put it in the top of the queue.
            match &req_inner.by_what {
                None => return Err(Status::invalid_argument("by_what needs to be specified")),
                Some(proto::get_directory_request::ByWhat::Digest(digest)) => {
                    let digest: [u8; 32] = digest
                        .as_slice()
                        .try_into()
                        .map_err(|_e| Status::invalid_argument("invalid digest length"))?;

                    let digest_b64: String = BASE64.encode(&digest);

                    task::spawn(async move {
                        if !req_inner.recursive {
                            let e: Result<proto::Directory, Status> =
                                match directory_service.get(&digest) {
                                    Ok(Some(directory)) => Ok(directory),
                                    Ok(None) => Err(Status::not_found(format!(
                                        "directory {} not found",
                                        digest_b64
                                    ))),
                                    Err(e) => Err(e.into()),
                                };

                            if tx.send(e).await.is_err() {
                                debug!("receiver dropped");
                            }
                        } else {
                            // If recursive was requested, traverse via get_recursive.
                            let directories_it = directory_service.get_recursive(&digest);

                            for e in directories_it {
                                // map err in res from Error to Status
                                let res = e.map_err(|e| Status::internal(e.to_string()));
                                if tx.send(res).await.is_err() {
                                    debug!("receiver dropped");
                                    break;
                                }
                            }
                        }
                    });
                }
            }
        };

        let receiver_stream = ReceiverStream::new(rx);
        Ok(Response::new(receiver_stream))
    }

    #[instrument(skip(self, request))]
    async fn put(
        &self,
        request: Request<Streaming<proto::Directory>>,
    ) -> Result<Response<proto::PutDirectoryResponse>, Status> {
        let mut req_inner = request.into_inner();
        // TODO: let this use DirectoryPutter to the store it's connected to,
        // and move the validation logic into [SimplePutter].

        // This keeps track of the seen directory keys, and their size.
        // This is used to validate the size field of a reference to a previously sent directory.
        // We don't need to keep the contents around, they're stored in the DB.
        let mut seen_directories_sizes: HashMap<[u8; 32], u32> = HashMap::new();
        let mut last_directory_dgst: Option<[u8; 32]> = None;

        // Consume directories, and insert them into the store.
        // Reject directory messages that refer to Directories not sent in the same stream.
        while let Some(directory) = req_inner.message().await? {
            // validate the directory itself.
            if let Err(e) = directory.validate() {
                return Err(Status::invalid_argument(format!(
                    "directory {} failed validation: {}",
                    BASE64.encode(&directory.digest()),
                    e,
                )));
            }

            // for each child directory this directory refers to, we need
            // to ensure it has been seen already in this stream, and that the size
            // matches what we recorded.
            for child_directory in &directory.directories {
                let child_directory_digest: [u8; 32] = child_directory
                    .digest
                    .clone()
                    .try_into()
                    .map_err(|_e| Status::internal("invalid child directory digest len"))?;

                match seen_directories_sizes.get(&child_directory_digest) {
                    None => {
                        return Err(Status::invalid_argument(format!(
                            "child directory '{}' ({}) in directory '{}' not seen yet",
                            child_directory.name,
                            BASE64.encode(&child_directory_digest),
                            BASE64.encode(&directory.digest()),
                        )));
                    }
                    Some(seen_child_directory_size) => {
                        if seen_child_directory_size != &child_directory.size {
                            return Err(Status::invalid_argument(format!(
                                    "child directory '{}' ({}) in directory '{}' referred with wrong size, expected {}, actual {}",
                                    child_directory.name,
                                    BASE64.encode(&child_directory_digest),
                                    BASE64.encode(&directory.digest()),
                                    seen_child_directory_size,
                                    child_directory.size,
                                )));
                        }
                    }
                }
            }

            // NOTE: We can't know if a directory we're receiving actually is
            // part of the closure, because we receive directories from the leaf nodes up to
            // the root.
            // The only thing we could to would be doing a final check when the
            // last Directory was received, that all Directories received so far are
            // reachable from that (root) node.

            let dgst = directory.digest();
            seen_directories_sizes.insert(dgst, directory.size());
            last_directory_dgst = Some(dgst);

            // check if the directory already exists in the database. We can skip
            // inserting if it's already there, as that'd be a no-op.
            match self.directory_service.get(&dgst) {
                Err(e) => {
                    warn!("error checking if directory already exists: {}", e);
                    return Err(e.into());
                }
                // skip if already exists
                Ok(Some(_)) => {}
                // insert if it doesn't already exist
                Ok(None) => {
                    self.directory_service.put(directory)?;
                }
            }
        }

        // We're done receiving. peek at last_directory_digest and either return the digest,
        // or an error, if we received an empty stream.
        match last_directory_dgst {
            None => Err(Status::invalid_argument("no directories received")),
            Some(last_directory_dgst) => Ok(Response::new(proto::PutDirectoryResponse {
                root_digest: last_directory_dgst.to_vec(),
            })),
        }
    }
}
