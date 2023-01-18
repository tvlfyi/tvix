use data_encoding::BASE64;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Sender;

use prost::Message;
use tokio::task;
use tokio_stream::wrappers::ReceiverStream;

use crate::proto::directory_service_server::DirectoryService;
use crate::proto::get_directory_request::ByWhat;
use crate::proto::Directory;
use crate::proto::GetDirectoryRequest;
use crate::proto::PutDirectoryResponse;
use tonic::{Request, Response, Result, Status, Streaming};
use tracing::{debug, instrument, warn};

pub struct SledDirectoryService {
    db: sled::Db,
}

impl SledDirectoryService {
    pub fn new(p: PathBuf) -> Result<Self, anyhow::Error> {
        let config = sled::Config::default().use_compression(true).path(p);
        let db = config.open()?;

        Ok(Self { db })
    }
}

/// Lookup a directory, optionally recurse, and send the result to the passed sender.
/// We pass in a txn, that has been opened on the outside.
/// It's up to the user to wrap this in a TransactionError appropriately.
/// This will open a sled txn to ensure a consistent view.
fn send_directories(
    txn: &sled::transaction::TransactionalTree,
    tx: &Sender<Result<Directory>>,
    req: GetDirectoryRequest,
) -> Result<(), Status> {
    // keep the list of directories to traverse
    let mut deq: VecDeque<Vec<u8>> = VecDeque::new();

    // look at the digest in the request and put it in the top of the queue.
    match &req.by_what {
        None => return Err(Status::invalid_argument("by_what needs to be specified")),
        Some(ByWhat::Digest(digest)) => {
            if digest.len() != 32 {
                return Err(Status::invalid_argument("invalid digest length"));
            }
            deq.push_back(digest.clone());
        }
    }

    // look up the directory at the top of the queue
    while let Some(ref digest) = deq.pop_front() {
        let digest_b64: String = BASE64.encode(digest);

        match txn.get(digest) {
            // The directory was not found, abort
            Ok(None) => {
                return Err(Status::not_found(format!(
                    "directory {} not found",
                    digest_b64
                )))
            }
            // The directory was found, try to parse the data as Directory message
            Ok(Some(data)) => match Directory::decode(&*data) {
                Err(e) => {
                    warn!("unable to parse directory {}: {}", digest_b64, e);
                    return Err(Status::internal(format!(
                        "unable to parse directory {}",
                        digest_b64
                    )));
                }
                Ok(directory) => {
                    // Validate the retrieved Directory indeed has the
                    // digest we expect it to have, to detect corruptions.
                    let actual_digest = directory.digest();
                    if actual_digest != digest.clone() {
                        return Err(Status::data_loss(format!(
                            "requested directory with digest {}, but got {}",
                            digest_b64,
                            BASE64.encode(&actual_digest)
                        )));
                    }

                    // if recursion was requested, all its children need to be added to the queue.
                    if req.recursive {
                        for child_directory_node in &directory.directories {
                            deq.push_back(child_directory_node.digest.clone());
                        }
                    }

                    // send the directory message to the client
                    if let Err(e) = tx.blocking_send(Ok(directory)) {
                        debug!("error sending: {}", e);
                        return Err(Status::internal("error sending"));
                    }
                }
            },
            // some storage error?
            Err(e) => {
                // TODO: check what this error really means
                warn!("storage error: {}", e);
                return Err(Status::internal("storage error"));
            }
        };
    }

    // nothing left, we're done
    Ok(())
}

/// Consume directories, and insert them into the database.
/// Reject directory messages that refer to Directories not sent in the same stream.
fn insert_directories(
    txn: &sled::transaction::TransactionalTree,
    directories: &Vec<Directory>,
) -> Result<Vec<u8>, Status> {
    // This keeps track of the seen directory keys.
    // We don't need to keep the contents around, they're stored in the DB.
    let mut seen_directory_dgsts: HashSet<Vec<u8>> = HashSet::new();
    let mut last_directory_dgst: Option<Vec<u8>> = None;

    for directory in directories {
        // for each child directory this directory refers to, we need
        // to ensure it has been seen already in this stream.
        for child_directory in &directory.directories {
            if !seen_directory_dgsts.contains(&child_directory.digest) {
                return Err(Status::invalid_argument(format!(
                    "referred child directory {} not seen yet",
                    BASE64.encode(&child_directory.digest)
                )));
            }
        }

        // TODO: do we want to verify this somehow is connected to the current graph?
        // theoretically, this currently allows uploading multiple
        // disconnected graphs at the same time…

        let dgst = directory.digest();
        seen_directory_dgsts.insert(dgst.clone());
        last_directory_dgst = Some(dgst.clone());

        // insert the directory into the database
        if let Err(e) = txn.insert(dgst, directory.encode_to_vec()) {
            match e {
                // TODO: conflict is a problem, as the whole transaction closure is retried?
                sled::transaction::UnabortableTransactionError::Conflict => todo!(),
                sled::transaction::UnabortableTransactionError::Storage(e) => {
                    warn!("storage error: {}", e);
                    return Err(Status::internal("storage error"));
                }
            }
        }
    }

    // no more directories to be send. Return the digest of the
    // last (root) node, or an error if we don't receive a single one.
    match last_directory_dgst {
        Some(last_directory_dgst) => Ok(last_directory_dgst),
        None => Err(Status::invalid_argument("no directories received")),
    }
}

#[tonic::async_trait]
impl DirectoryService for SledDirectoryService {
    type GetStream = ReceiverStream<Result<Directory>>;

    #[instrument(skip(self))]
    async fn get(
        &self,
        request: Request<GetDirectoryRequest>,
    ) -> Result<Response<Self::GetStream>, Status> {
        let (tx, rx) = channel(5);

        let req_inner = request.into_inner();

        // clone self.db (cheap), so we don't refer to self in the thread.
        let db = self.db.clone();

        // kick off a thread
        task::spawn_blocking(move || {
            // open a DB transaction.
            let txn_res = db.transaction(|txn| {
                send_directories(txn, &tx, req_inner.clone())
                    .map_err(sled::transaction::ConflictableTransactionError::Abort)
            });

            // handle transaction errors
            match txn_res {
                Ok(()) => Ok(()),
                Err(sled::transaction::TransactionError::Abort(status)) => {
                    // if the transaction was aborted, there was an error. Send it to the client
                    tx.blocking_send(Err(status))
                }
                Err(sled::transaction::TransactionError::Storage(e)) => {
                    warn!("storage error: {}", e);
                    tx.blocking_send(Err(Status::internal("storage error")))
                }
            }
        });

        // NOTE: this always returns an Ok response, with the first item in the
        // stream being a potential error, instead of directly returning the
        // first error.
        // Peeking on the first element seems to be extraordinarily hard,
        // and client code considers these two equivalent anyways.
        let receiver_stream = ReceiverStream::new(rx);
        Ok(Response::new(receiver_stream))
    }

    #[instrument(skip(self, request))]
    async fn put(
        &self,
        request: Request<Streaming<Directory>>,
    ) -> Result<Response<PutDirectoryResponse>> {
        let mut req_inner = request.into_inner();

        // TODO: for now, we collect all Directory messages into a Vec, and
        // pass it to the helper function.
        // Ideally, we would validate things as we receive it, and have a way
        // to reject early - but the semantics of transactions (and its
        // retries) don't allow us to discard things that early anyways.
        let mut directories: Vec<Directory> = Vec::new();

        while let Some(directory) = req_inner.message().await? {
            directories.push(directory)
        }

        let txn_res = self.db.transaction(|txn| {
            insert_directories(txn, &directories)
                .map_err(sled::transaction::ConflictableTransactionError::Abort)
        });

        match txn_res {
            Ok(last_directory_dgst) => Ok(Response::new(PutDirectoryResponse {
                root_digest: last_directory_dgst,
            })),
            Err(sled::transaction::TransactionError::Storage(e)) => {
                warn!("storage error: {}", e);
                Err(Status::internal("storage error"))
            }
            Err(sled::transaction::TransactionError::Abort(e)) => Err(e),
        }
    }
}
