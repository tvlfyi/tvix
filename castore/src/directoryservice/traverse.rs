use crate::{directoryservice::DirectoryService, Error, Node, Path};
use tracing::{instrument, warn};

/// This descends from a (root) node to the given (sub)path, returning the Node
/// at that path, or none, if there's nothing at that path.
#[instrument(skip(directory_service, path), fields(%path))]
pub async fn descend_to<DS>(
    directory_service: DS,
    root_node: Node,
    path: impl AsRef<Path> + std::fmt::Display,
) -> Result<Option<Node>, Error>
where
    DS: AsRef<dyn DirectoryService>,
{
    let mut parent_node = root_node;
    for component in path.as_ref().components() {
        match parent_node {
            Node::File { .. } | Node::Symlink { .. } => {
                // There's still some path left, but the parent node is no directory.
                // This means the path doesn't exist, as we can't reach it.
                return Ok(None);
            }
            Node::Directory { digest, .. } => {
                // fetch the linked node from the directory_service.
                let directory =
                    directory_service
                        .as_ref()
                        .get(&digest)
                        .await?
                        .ok_or_else(|| {
                            // If we didn't get the directory node that's linked, that's a store inconsistency, bail out!
                            warn!("directory {} does not exist", digest);

                            Error::StorageError(format!("directory {} does not exist", digest))
                        })?;

                // look for the component in the [Directory].
                if let Some((_child_name, child_node)) = directory
                    .nodes()
                    .find(|(name, _node)| name.as_ref() == component)
                {
                    // child node found, update prev_node to that and continue.
                    parent_node = child_node.clone();
                } else {
                    // child node not found means there's no such element inside the directory.
                    return Ok(None);
                };
            }
        }
    }

    // We traversed the entire path, so this must be the node.
    Ok(Some(parent_node))
}

#[cfg(test)]
mod tests {
    use crate::{
        directoryservice,
        fixtures::{DIRECTORY_COMPLICATED, DIRECTORY_WITH_KEEP, EMPTY_BLOB_DIGEST},
        Node, PathBuf,
    };

    use super::descend_to;

    #[tokio::test]
    async fn test_descend_to() {
        let directory_service = directoryservice::from_addr("memory://").await.unwrap();

        let mut handle = directory_service.put_multiple_start();
        handle
            .put(DIRECTORY_WITH_KEEP.clone())
            .await
            .expect("must succeed");
        handle
            .put(DIRECTORY_COMPLICATED.clone())
            .await
            .expect("must succeed");

        handle.close().await.expect("must upload");

        // construct the node for DIRECTORY_COMPLICATED
        let node_directory_complicated = Node::Directory {
            digest: DIRECTORY_COMPLICATED.digest(),
            size: DIRECTORY_COMPLICATED.size(),
        };

        // construct the node for DIRECTORY_COMPLICATED
        let node_directory_with_keep = Node::Directory {
            digest: DIRECTORY_WITH_KEEP.digest(),
            size: DIRECTORY_WITH_KEEP.size(),
        };

        // construct the node for the .keep file
        let node_file_keep = Node::File {
            digest: EMPTY_BLOB_DIGEST.clone(),
            size: 0,
            executable: false,
        };

        // traversal to an empty subpath should return the root node.
        {
            let resp = descend_to(
                &directory_service,
                node_directory_complicated.clone(),
                "".parse::<PathBuf>().unwrap(),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_directory_complicated.clone()), resp);
        }

        // traversal to `keep` should return the node for DIRECTORY_WITH_KEEP
        {
            let resp = descend_to(
                &directory_service,
                node_directory_complicated.clone(),
                "keep".parse::<PathBuf>().unwrap(),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_directory_with_keep), resp);
        }

        // traversal to `keep/.keep` should return the node for the .keep file
        {
            let resp = descend_to(
                &directory_service,
                node_directory_complicated.clone(),
                "keep/.keep".parse::<PathBuf>().unwrap(),
            )
            .await
            .expect("must succeed");

            assert_eq!(Some(node_file_keep.clone()), resp);
        }

        // traversal to `void` should return None (doesn't exist)
        {
            let resp = descend_to(
                &directory_service,
                node_directory_complicated.clone(),
                "void".parse::<PathBuf>().unwrap(),
            )
            .await
            .expect("must succeed");

            assert_eq!(None, resp);
        }

        // traversal to `v/oid` should return None (doesn't exist)
        {
            let resp = descend_to(
                &directory_service,
                node_directory_complicated.clone(),
                "v/oid".parse::<PathBuf>().unwrap(),
            )
            .await
            .expect("must succeed");

            assert_eq!(None, resp);
        }

        // traversal to `keep/.keep/404` should return None (the path can't be
        // reached, as keep/.keep already is a file)
        {
            let resp = descend_to(
                &directory_service,
                node_directory_complicated.clone(),
                "keep/.keep/foo".parse::<PathBuf>().unwrap(),
            )
            .await
            .expect("must succeed");

            assert_eq!(None, resp);
        }
    }
}
