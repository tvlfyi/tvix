use super::DirectoryPutter;
use super::DirectoryService;
use crate::proto;
use crate::B3Digest;
use crate::Error;
use std::collections::{HashSet, VecDeque};
use tracing::{debug_span, instrument, warn};

/// Traverses a [proto::Directory] from the root to the children.
///
/// This is mostly BFS, but directories are only returned once.
pub struct DirectoryTraverser<DS: DirectoryService> {
    directory_service: DS,
    /// The list of all directories that still need to be traversed. The next
    /// element is picked from the front, new elements are enqueued at the
    /// back.
    worklist_directory_digests: VecDeque<B3Digest>,
    /// The list of directory digests already sent to the consumer.
    /// We omit sending the same directories multiple times.
    sent_directory_digests: HashSet<B3Digest>,
}

impl<DS: DirectoryService> DirectoryTraverser<DS> {
    pub fn with(directory_service: DS, root_directory_digest: &B3Digest) -> Self {
        Self {
            directory_service,
            worklist_directory_digests: VecDeque::from([root_directory_digest.clone()]),
            sent_directory_digests: HashSet::new(),
        }
    }

    // enqueue all child directory digests to the work queue, as
    // long as they're not part of the worklist or already sent.
    // This panics if the digest looks invalid, it's supposed to be checked first.
    fn enqueue_child_directories(&mut self, directory: &proto::Directory) {
        for child_directory_node in &directory.directories {
            // TODO: propagate error
            let child_digest: B3Digest = child_directory_node.digest.clone().try_into().unwrap();

            if self.worklist_directory_digests.contains(&child_digest)
                || self.sent_directory_digests.contains(&child_digest)
            {
                continue;
            }
            self.worklist_directory_digests.push_back(child_digest);
        }
    }
}

impl<DS: DirectoryService> Iterator for DirectoryTraverser<DS> {
    type Item = Result<proto::Directory, Error>;

    #[instrument(skip_all)]
    fn next(&mut self) -> Option<Self::Item> {
        // fetch the next directory digest from the top of the work queue.
        match self.worklist_directory_digests.pop_front() {
            None => None,
            Some(current_directory_digest) => {
                let span = debug_span!("directory.digest", "{}", current_directory_digest);
                let _ = span.enter();

                // look up the directory itself.
                let current_directory = match self.directory_service.get(&current_directory_digest)
                {
                    // if we got it
                    Ok(Some(current_directory)) => {
                        // validate, we don't want to send invalid directories.
                        if let Err(e) = current_directory.validate() {
                            warn!("directory failed validation: {}", e.to_string());
                            return Some(Err(Error::StorageError(format!(
                                "invalid directory: {}",
                                current_directory_digest
                            ))));
                        }
                        current_directory
                    }
                    // if it's not there, we have an inconsistent store!
                    Ok(None) => {
                        warn!("directory {} does not exist", current_directory_digest);
                        return Some(Err(Error::StorageError(format!(
                            "directory {} does not exist",
                            current_directory_digest
                        ))));
                    }
                    Err(e) => {
                        warn!("failed to look up directory");
                        return Some(Err(Error::StorageError(format!(
                            "unable to look up directory {}: {}",
                            current_directory_digest, e
                        ))));
                    }
                };

                // All DirectoryServices MUST validate directory nodes, before returning them out, so we
                // can be sure [enqueue_child_directories] doesn't panic.

                // enqueue child directories
                self.enqueue_child_directories(&current_directory);
                Some(Ok(current_directory))
            }
        }
    }
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

impl<DS: DirectoryService> DirectoryPutter for SimplePutter<DS> {
    fn put(&mut self, directory: proto::Directory) -> Result<(), Error> {
        if self.closed {
            return Err(Error::StorageError("already closed".to_string()));
        }

        let digest = self.directory_service.put(directory)?;

        // track the last directory digest
        self.last_directory_digest = Some(digest);

        Ok(())
    }

    /// We need to be mutable here, as that's the signature of the trait.
    fn close(&mut self) -> Result<B3Digest, Error> {
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

    fn is_closed(&self) -> bool {
        self.closed
    }
}
