use super::DirectoryService;
use crate::proto;
use crate::B3Digest;
use crate::Error;
use async_stream::try_stream;
use futures::stream::BoxStream;
use std::collections::{HashSet, VecDeque};
use tracing::instrument;
use tracing::warn;

/// Traverses a [proto::Directory] from the root to the children.
///
/// This is mostly BFS, but directories are only returned once.
#[instrument(skip(directory_service))]
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

    let root_directory_digest = root_directory_digest.clone();

    Box::pin(try_stream! {
        while let Some(current_directory_digest) = worklist_directory_digests.pop_front() {
            let current_directory = match directory_service.get(&current_directory_digest).await.map_err(|e| {
                warn!("failed to look up directory");
                Error::StorageError(format!(
                    "unable to look up directory {}: {}",
                    current_directory_digest, e
                ))
            })? {
                // the root node of the requested closure was not found, return an empty list
                None if current_directory_digest == root_directory_digest => break,
                // if a child directory of the closure is not there, we have an inconsistent store!
                None => {
                    warn!("directory {} does not exist", current_directory_digest);
                    Err(Error::StorageError(format!(
                        "directory {} does not exist",
                        current_directory_digest
                    )))?;
                    break;
                }
                Some(dir) => dir,
            };


            // validate, we don't want to send invalid directories.
            current_directory.validate().map_err(|e| {
               warn!("directory failed validation: {}", e.to_string());
               Error::StorageError(format!(
                   "invalid directory: {}",
                   current_directory_digest
               ))
            })?;

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

            yield current_directory;
        }
    })
}
