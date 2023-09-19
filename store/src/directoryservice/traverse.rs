use super::DirectoryService;
use crate::{proto::NamedNode, B3Digest, Error};
use std::{os::unix::ffi::OsStrExt, sync::Arc};
use tracing::{instrument, warn};

/// This traverses from a (root) node to the given (sub)path, returning the Node
/// at that path, or none, if there's nothing at that path.
/// TODO: Do we want to use [DirectoryService.get_recursive] to do less lookups?
/// Or do we consider this to be a non-issue due to store composition and local caching?
/// TODO: the name of this function (and mod) is a bit bad, because it doesn't
/// clearly distinguish it from the BFS traversers.
#[instrument(skip(directory_service))]
pub async fn traverse_to(
    directory_service: Arc<dyn DirectoryService>,
    root_node: crate::proto::node::Node,
    path: &std::path::Path,
) -> Result<Option<crate::proto::node::Node>, Error> {
    // strip a possible `/` prefix from the path.
    let path = {
        if path.starts_with("/") {
            path.strip_prefix("/").unwrap()
        } else {
            path
        }
    };

    let mut cur_node = root_node;
    let mut it = path.components();

    loop {
        match it.next() {
            None => {
                // the (remaining) path is empty, return the node we're current at.
                return Ok(Some(cur_node));
            }
            Some(first_component) => {
                match cur_node {
                    crate::proto::node::Node::File(_) | crate::proto::node::Node::Symlink(_) => {
                        // There's still some path left, but the current node is no directory.
                        // This means the path doesn't exist, as we can't reach it.
                        return Ok(None);
                    }
                    crate::proto::node::Node::Directory(directory_node) => {
                        let digest: B3Digest = directory_node.digest.try_into().map_err(|_e| {
                            Error::StorageError("invalid digest length".to_string())
                        })?;

                        // fetch the linked node from the directory_service
                        match directory_service.get(&digest).await? {
                            // If we didn't get the directory node that's linked, that's a store inconsistency, bail out!
                            None => {
                                warn!("directory {} does not exist", digest);

                                return Err(Error::StorageError(format!(
                                    "directory {} does not exist",
                                    digest
                                )));
                            }
                            Some(directory) => {
                                // look for first_component in the [Directory].
                                // FUTUREWORK: as the nodes() iterator returns in a sorted fashion, we
                                // could stop as soon as e.name is larger than the search string.
                                let child_node = directory.nodes().find(|n| {
                                    n.get_name() == first_component.as_os_str().as_bytes()
                                });

                                match child_node {
                                    // child node not found means there's no such element inside the directory.
                                    None => {
                                        return Ok(None);
                                    }
                                    // child node found, return to top-of loop to find the next
                                    // node in the path.
                                    Some(child_node) => {
                                        cur_node = child_node;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::tests::{
        fixtures::{DIRECTORY_COMPLICATED, DIRECTORY_WITH_KEEP},
        utils::gen_directory_service,
    };

    use super::traverse_to;

    #[tokio::test]
    async fn test_traverse_to() {
        let directory_service = gen_directory_service();

        let mut handle = directory_service.put_multiple_start();
        handle
            .put(DIRECTORY_WITH_KEEP.clone())
            .await
            .expect("must succeed");
        handle
            .put(DIRECTORY_COMPLICATED.clone())
            .await
            .expect("must succeed");

        // construct the node for DIRECTORY_COMPLICATED
        let node_directory_complicated =
            crate::proto::node::Node::Directory(crate::proto::DirectoryNode {
                name: "doesntmatter".into(),
                digest: DIRECTORY_COMPLICATED.digest().into(),
                size: DIRECTORY_COMPLICATED.size(),
            });

        // construct the node for DIRECTORY_COMPLICATED
        let node_directory_with_keep = crate::proto::node::Node::Directory(
            DIRECTORY_COMPLICATED.directories.first().unwrap().clone(),
        );

        // construct the node for the .keep file
        let node_file_keep =
            crate::proto::node::Node::File(DIRECTORY_WITH_KEEP.files.first().unwrap().clone());

        // traversal to an empty subpath should return the root node.
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from(""),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_directory_complicated.clone()), resp);
        }

        // traversal to `keep` should return the node for DIRECTORY_WITH_KEEP
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from("keep"),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_directory_with_keep), resp);
        }

        // traversal to `keep/.keep` should return the node for the .keep file
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from("keep/.keep"),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_file_keep.clone()), resp);
        }

        // traversal to `keep/.keep` should return the node for the .keep file
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from("/keep/.keep"),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_file_keep), resp);
        }

        // traversal to `void` should return None (doesn't exist)
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from("void"),
            )
            .await
            .expect("must succeed");

            assert_eq!(None, resp);
        }

        // traversal to `void` should return None (doesn't exist)
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from("//v/oid"),
            )
            .await
            .expect("must succeed");

            assert_eq!(None, resp);
        }

        // traversal to `keep/.keep/404` should return None (the path can't be
        // reached, as keep/.keep already is a file)
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from("keep/.keep/foo"),
            )
            .await
            .expect("must succeed");

            assert_eq!(None, resp);
        }

        // traversal to a subpath of '/' should return the root node.
        {
            let resp = traverse_to(
                directory_service.clone(),
                node_directory_complicated.clone(),
                &PathBuf::from("/"),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_directory_complicated), resp);
        }
    }
}
