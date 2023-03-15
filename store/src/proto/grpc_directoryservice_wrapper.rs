use crate::directoryservice::DirectoryService;
use crate::proto;
use data_encoding::BASE64;
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::{sync::mpsc::channel, task};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::{debug, info_span, instrument, warn};

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

        // kick off an async thread
        task::spawn(async move {
            // Keep the list of directory digests to traverse.
            // As per rpc_directory.proto, we traverse in BFS order.
            let mut deq: VecDeque<[u8; 32]> = VecDeque::new();

            // look at the digest in the request and put it in the top of the queue.
            match &req_inner.by_what {
                None => return Err(Status::invalid_argument("by_what needs to be specified")),
                Some(proto::get_directory_request::ByWhat::Digest(digest)) => {
                    deq.push_back(
                        digest
                            .as_slice()
                            .try_into()
                            .map_err(|_e| Status::invalid_argument("invalid digest length"))?,
                    );
                }
            }

            // keep a list of all the Directory messages already sent, so we can omit sending the same.
            let mut sent_directory_dgsts: HashSet<[u8; 32]> = HashSet::new();

            // look up the directory at the top of the queue
            while let Some(digest) = deq.pop_front() {
                let digest_b64: String = BASE64.encode(&digest);

                // add digest we're currently processing to a span, but pay attention to
                // https://docs.rs/tracing/0.1.37/tracing/span/struct.Span.html#in-asynchronous-code
                // There may be no await in here (we leave the span before the tx.send(…).await)
                let span = info_span!("digest", "{}", &digest_b64);

                let res: Result<proto::Directory, Status> = {
                    let _enter = span.enter();

                    // invoke client.get, and map to a Result<Directory, Status>
                    match directory_service.get(&digest) {
                        // The directory was not found, abort
                        Ok(None) => {
                            if !sent_directory_dgsts.is_empty() {
                                // If this is not the first lookup, we have a
                                // consistency issue, and we're missing some children, of which we have the
                                // parents. Log this out.
                                // Both the node we started with, and the
                                // current digest are part of the span.
                                warn!("consistency issue: directory not found")
                            }
                            Err(Status::not_found(format!(
                                "directory {} not found",
                                digest_b64
                            )))
                        }
                        Ok(Some(directory)) => {
                            // if recursion was requested, all its children need to be added to the queue.
                            // If a Directory message with the same digest has already
                            // been sent previously, we can skip enqueueing it.
                            // Same applies to when it already is in the queue.
                            if req_inner.recursive {
                                for child_directory_node in &directory.directories {
                                    let child_directory_node_digest: [u8; 32] =
                                        child_directory_node.digest.clone().try_into().map_err(
                                            |_e| {
                                                Status::internal(
                                                    "invalid child directory digest len",
                                                )
                                            },
                                        )?;

                                    if !sent_directory_dgsts.contains(&child_directory_node_digest)
                                        && !deq.contains(&child_directory_node_digest)
                                    {
                                        deq.push_back(child_directory_node_digest);
                                    }
                                }
                            }

                            // add it to sent_directory_dgsts.
                            // Strictly speaking, it wasn't sent yet, but tx.send happens right after,
                            // and the only way we can still fail is by the remote side to hang up,
                            // in which case we stop anyways.
                            sent_directory_dgsts.insert(digest);

                            Ok(directory)
                        }
                        Err(e) => Err(e.into()),
                    }
                };

                // send the result to the client
                if (tx.send(res).await).is_err() {
                    debug!("receiver dropped");
                    break;
                }
            }

            Ok(())
        });

        // NOTE: this always returns an Ok response, with the first item in the
        // stream being a potential error, instead of directly returning the
        // first error.
        // There's no need to check if the directory node exists twice,
        // and client code should consider an Err(), or the first item of the
        // stream being an error to be equivalent.
        let receiver_stream = ReceiverStream::new(rx);
        Ok(Response::new(receiver_stream))
    }

    #[instrument(skip(self, request))]
    async fn put(
        &self,
        request: Request<Streaming<proto::Directory>>,
    ) -> Result<Response<proto::PutDirectoryResponse>, Status> {
        let mut req_inner = request.into_inner();
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

            // TODO: We don't validate the currently received directory refers
            // to at least one child we already received.
            // This means, we thoeretically allow uploading multiple disconnected graphs,
            // and the digest of the last element in the stream becomes the root node.
            // For example, you can upload a leaf directory A, a leaf directory
            // B, and then as last element a directory C that only refers to A,
            // leaving B disconnected.
            // At some point, we might want to populate a datastructure that
            // does a reachability check.

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
