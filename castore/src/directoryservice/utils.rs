use super::DirectoryPutter;
use super::DirectoryService;
use crate::proto;
use crate::B3Digest;
use crate::Error;
use async_stream::stream;
use futures::stream::BoxStream;
use std::collections::{HashSet, VecDeque};
use tonic::async_trait;
use tracing::warn;

/// Traverses a [proto::Directory] from the root to the children.
///
/// This is mostly BFS, but directories are only returned once.
pub fn traverse_directory<'a, DS: DirectoryService + 'static>(
    directory_service: DS,
    root_directory_digest: &B3Digest,
) -> BoxStream<'a, Result<proto::Directory, Error>> {
    // The list of all directories that still need to be traversed. The next
    // element is picked from the front, new elements are enqueued at the
    // back.
    let mut worklist_directory_digests: VecDeque<B3Digest> =
        VecDeque::from([root_directory_digest.clone()]);
    // The list of directory digests already sent to the consumer.
    // We omit sending the same directories multiple times.
    let mut sent_directory_digests: HashSet<B3Digest> = HashSet::new();

    let stream = stream! {
        while let Some(current_directory_digest) = worklist_directory_digests.pop_front() {
            match directory_service.get(&current_directory_digest).await {
                // if it's not there, we have an inconsistent store!
                Ok(None) => {
                    warn!("directory {} does not exist", current_directory_digest);
                    yield Err(Error::StorageError(format!(
                        "directory {} does not exist",
                        current_directory_digest
                    )));
                }
                Err(e) => {
                    warn!("failed to look up directory");
                    yield Err(Error::StorageError(format!(
                        "unable to look up directory {}: {}",
                        current_directory_digest, e
                    )));
                }

                // if we got it
                Ok(Some(current_directory)) => {
                    // validate, we don't want to send invalid directories.
                    if let Err(e) = current_directory.validate() {
                        warn!("directory failed validation: {}", e.to_string());
                        yield Err(Error::StorageError(format!(
                            "invalid directory: {}",
                            current_directory_digest
                        )));
                    }

                    // We're about to send this directory, so let's avoid sending it again if a
                    // descendant has it.
                    sent_directory_digests.insert(current_directory_digest);

                    // enqueue all child directory digests to the work queue, as
                    // long as they're not part of the worklist or already sent.
                    // This panics if the digest looks invalid, it's supposed to be checked first.
                    for child_directory_node in &current_directory.directories {
                        // TODO: propagate error
                        let child_digest: B3Digest = child_directory_node.digest.clone().try_into().unwrap();

                        if worklist_directory_digests.contains(&child_digest)
                            || sent_directory_digests.contains(&child_digest)
                        {
                            continue;
                        }
                        worklist_directory_digests.push_back(child_digest);
                    }

                    yield Ok(current_directory);
                }
            };
        }
    };

    Box::pin(stream)
}

/// This is a simple implementation of a Directory uploader.
/// TODO: verify connectivity? Factor out these checks into generic helpers?
pub struct SimplePutter<DS: DirectoryService> {
    directory_service: DS,
    last_directory_digest: Option<B3Digest>,
    closed: bool,
}

impl<DS: DirectoryService> SimplePutter<DS> {
    pub fn new(directory_service: DS) -> Self {
        Self {
            directory_service,
            closed: false,
            last_directory_digest: None,
        }
    }
}

#[async_trait]
impl<DS: DirectoryService + 'static> DirectoryPutter for SimplePutter<DS> {
    async fn put(&mut self, directory: proto::Directory) -> Result<(), Error> {
        if self.closed {
            return Err(Error::StorageError("already closed".to_string()));
        }

        let digest = self.directory_service.put(directory).await?;

        // track the last directory digest
        self.last_directory_digest = Some(digest);

        Ok(())
    }

    async fn close(&mut self) -> Result<B3Digest, Error> {
        if self.closed {
            return Err(Error::StorageError("already closed".to_string()));
        }

        match &self.last_directory_digest {
            Some(last_digest) => {
                self.closed = true;
                Ok(last_digest.clone())
            }
            None => Err(Error::InvalidRequest(
                "no directories sent, can't show root digest".to_string(),
            )),
        }
    }
}
