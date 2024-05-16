use std::collections::{HashMap, HashSet};

use bstr::ByteSlice;

use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::{Bfs, Walker},
};
use tracing::instrument;

use crate::{
    proto::{self, Directory},
    B3Digest, Error,
};

type DirectoryGraph = DiGraph<Directory, ()>;

/// This can be used to validate a Directory closure (DAG of connected
/// Directories), and their insertion order.
///
/// Directories need to be inserted (via `add`), in an order from the leaves to
/// the root (DFS Post-Order).
/// During insertion, We validate as much as we can at that time:
///
///  - individual validation of Directory messages
///  - validation of insertion order (no upload of not-yet-known Directories)
///  - validation of size fields of referred Directories
///
/// Internally it keeps all received Directories in a directed graph,
/// with node weights being the Directories and edges pointing to child
/// directories.
///
/// Once all Directories have been inserted, a finalize function can be
/// called to get a (deduplicated and) validated list of directories, in
/// insertion order.
/// During finalize, a check for graph connectivity is performed too, to ensure
/// there's no disconnected components, and only one root.
#[derive(Default)]
pub struct ClosureValidator {
    // A directed graph, using Directory as node weight, without edge weights.
    // Edges point from parents to children.
    graph: DirectoryGraph,

    // A lookup table from directory digest to node index.
    digest_to_node_ix: HashMap<B3Digest, NodeIndex>,

    /// Keeps track of the last-inserted directory graph node index.
    /// On a correct insert, this will be the root node, from which the DFS post
    /// order traversal will start from.
    last_directory_ix: Option<NodeIndex>,
}

impl ClosureValidator {
    /// Insert a new Directory into the closure.
    /// Perform individual Directory validation, validation of insertion order
    /// and size fields.
    #[instrument(level = "trace", skip_all, fields(directory.digest=%directory.digest(), directory.size=%directory.size()), err)]
    pub fn add(&mut self, directory: proto::Directory) -> Result<(), Error> {
        let digest = directory.digest();

        // If we already saw this node previously, it's already validated and in the graph.
        if self.digest_to_node_ix.contains_key(&digest) {
            return Ok(());
        }

        // Do some general validation
        directory
            .validate()
            .map_err(|e| Error::InvalidRequest(e.to_string()))?;

        // Ensure the directory only refers to directories which we already accepted.
        // We lookup their node indices and add them to a HashSet.
        let mut child_ixs = HashSet::new();
        for dir in &directory.directories {
            let child_digest = B3Digest::try_from(dir.digest.to_owned()).unwrap(); // validated

            // Ensure the digest has already been seen
            let child_ix = *self.digest_to_node_ix.get(&child_digest).ok_or_else(|| {
                Error::InvalidRequest(format!(
                    "'{}' refers to unseen child dir: {}",
                    dir.name.as_bstr(),
                    &child_digest
                ))
            })?;

            // Ensure the size specified in the child node matches the directory size itself.
            let recorded_child_size = self
                .graph
                .node_weight(child_ix)
                .expect("node not found")
                .size();

            // Ensure the size specified in the child node matches our records.
            if dir.size != recorded_child_size {
                return Err(Error::InvalidRequest(format!(
                    "'{}' has wrong size, specified {}, recorded {}",
                    dir.name.as_bstr(),
                    dir.size,
                    recorded_child_size
                )));
            }

            child_ixs.insert(child_ix);
        }

        // Insert node into the graph, and add edges to all children.
        let node_ix = self.graph.add_node(directory);
        for child_ix in child_ixs {
            self.graph.add_edge(node_ix, child_ix, ());
        }

        // Record the mapping from digest to node_ix in our lookup table.
        self.digest_to_node_ix.insert(digest, node_ix);

        // Update last_directory_ix.
        self.last_directory_ix = Some(node_ix);

        Ok(())
    }

    /// Ensure that all inserted Directories are connected, then return a
    /// (deduplicated) and validated list of directories, in from-leaves-to-root
    /// order.
    /// In case no elements have been inserted, returns an empty list.
    #[instrument(level = "trace", skip_all, err)]
    pub(crate) fn finalize(self) -> Result<Vec<Directory>, Error> {
        let (graph, _) = match self.finalize_raw()? {
            None => return Ok(vec![]),
            Some(v) => v,
        };
        // Dissolve the graph, returning the nodes as a Vec.
        // As the graph was populated in a valid DFS PostOrder, we can return
        // nodes in that same order.
        let (nodes, _edges) = graph.into_nodes_edges();
        Ok(nodes.into_iter().map(|x| x.weight).collect())
    }

    /// Ensure that all inserted Directories are connected, then return a
    /// (deduplicated) and validated list of directories, in from-root-to-leaves
    /// order.
    /// In case no elements have been inserted, returns an empty list.
    #[instrument(level = "trace", skip_all, err)]
    pub(crate) fn finalize_root_to_leaves(self) -> Result<Vec<Directory>, Error> {
        let (mut graph, root) = match self.finalize_raw()? {
            None => return Ok(vec![]),
            Some(v) => v,
        };

        // do a BFS traversal of the graph, starting with the root node to get
        // (the count of) all nodes reachable from there.
        let traversal = Bfs::new(&graph, root);

        Ok(traversal
            .iter(&graph)
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|i| graph.remove_node(i))
            .collect())
    }

    /// Internal implementation of closure validation
    #[instrument(level = "trace", skip_all, err)]
    fn finalize_raw(self) -> Result<Option<(DirectoryGraph, NodeIndex)>, Error> {
        // If no nodes were inserted, an empty list is returned.
        let last_directory_ix = if let Some(x) = self.last_directory_ix {
            x
        } else {
            return Ok(None);
        };

        // do a BFS traversal of the graph, starting with the root node to get
        // (the count of) all nodes reachable from there.
        let mut traversal = Bfs::new(&self.graph, last_directory_ix);

        let mut visited_directory_count = 0;
        #[cfg(debug_assertions)]
        let mut visited_directory_ixs = HashSet::new();
        #[cfg_attr(not(debug_assertions), allow(unused))]
        while let Some(directory_ix) = traversal.next(&self.graph) {
            #[cfg(debug_assertions)]
            visited_directory_ixs.insert(directory_ix);

            visited_directory_count += 1;
        }

        // If the number of nodes collected equals the total number of nodes in
        // the graph, we know all nodes are connected.
        if visited_directory_count != self.graph.node_count() {
            // more or less exhaustive error reporting.
            #[cfg(debug_assertions)]
            {
                let all_directory_ixs: HashSet<_> = self.graph.node_indices().collect();

                let unvisited_directories: HashSet<_> = all_directory_ixs
                    .difference(&visited_directory_ixs)
                    .map(|ix| self.graph.node_weight(*ix).expect("node not found"))
                    .collect();

                return Err(Error::InvalidRequest(format!(
                    "found {} disconnected directories: {:?}",
                    self.graph.node_count() - visited_directory_ixs.len(),
                    unvisited_directories
                )));
            }
            #[cfg(not(debug_assertions))]
            {
                return Err(Error::InvalidRequest(format!(
                    "found {} disconnected directories",
                    self.graph.node_count() - visited_directory_count
                )));
            }
        }

        Ok(Some((self.graph, last_directory_ix)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        fixtures::{DIRECTORY_A, DIRECTORY_B, DIRECTORY_C},
        proto::{self, Directory},
    };
    use lazy_static::lazy_static;
    use rstest::rstest;

    lazy_static! {
        pub static ref BROKEN_DIRECTORY : Directory = Directory {
            symlinks: vec![proto::SymlinkNode {
                name: "".into(), // invalid name!
                target: "doesntmatter".into(),
            }],
            ..Default::default()
        };

        pub static ref BROKEN_PARENT_DIRECTORY: Directory = Directory {
            directories: vec![proto::DirectoryNode {
                name: "foo".into(),
                digest: DIRECTORY_A.digest().into(),
                size: DIRECTORY_A.size() + 42, // wrong!
            }],
            ..Default::default()
        };
    }

    use super::ClosureValidator;

    #[rstest]
    /// Uploading an empty directory should succeed.
    #[case::empty_directory(&[&*DIRECTORY_A], false, Some(vec![&*DIRECTORY_A]))]
    /// Uploading A, then B (referring to A) should succeed.
    #[case::simple_closure(&[&*DIRECTORY_A, &*DIRECTORY_B], false, Some(vec![&*DIRECTORY_A, &*DIRECTORY_B]))]
    /// Uploading A, then A, then C (referring to A twice) should succeed.
    /// We pretend to be a dumb client not deduping directories.
    #[case::same_child(&[&*DIRECTORY_A, &*DIRECTORY_A, &*DIRECTORY_C], false, Some(vec![&*DIRECTORY_A, &*DIRECTORY_C]))]
    /// Uploading A, then C (referring to A twice) should succeed.
    #[case::same_child_dedup(&[&*DIRECTORY_A, &*DIRECTORY_C], false, Some(vec![&*DIRECTORY_A, &*DIRECTORY_C]))]
    /// Uploading A, then C (referring to A twice), then B (itself referring to A) should fail during close,
    /// as B itself would be left unconnected.
    #[case::unconnected_node(&[&*DIRECTORY_A, &*DIRECTORY_C, &*DIRECTORY_B], false, None)]
    /// Uploading B (referring to A) should fail immediately, because A was never uploaded.
    #[case::dangling_pointer(&[&*DIRECTORY_B], true, None)]
    /// Uploading a directory failing validation should fail immediately.
    #[case::failing_validation(&[&*BROKEN_DIRECTORY], true, None)]
    /// Uploading a directory which refers to another Directory with a wrong size should fail.
    #[case::wrong_size_in_parent(&[&*DIRECTORY_A, &*BROKEN_PARENT_DIRECTORY], true, None)]
    fn test_uploads(
        #[case] directories_to_upload: &[&Directory],
        #[case] exp_fail_upload_last: bool,
        #[case] exp_finalize: Option<Vec<&Directory>>, // Some(_) if finalize successful, None if not.
    ) {
        let mut dcv = ClosureValidator::default();
        let len_directories_to_upload = directories_to_upload.len();

        for (i, d) in directories_to_upload.iter().enumerate() {
            let resp = dcv.add((*d).clone());
            if i == len_directories_to_upload - 1 && exp_fail_upload_last {
                assert!(resp.is_err(), "expect last put to fail");

                // We don't really care anymore what finalize() would return, as
                // the add() failed.
                return;
            } else {
                assert!(resp.is_ok(), "expect put to succeed");
            }
        }

        // everything was uploaded successfully. Test finalize().
        let resp = dcv.finalize();

        match exp_finalize {
            Some(directories) => {
                assert_eq!(
                    Vec::from_iter(directories.iter().map(|e| (*e).to_owned())),
                    resp.expect("drain should succeed")
                );
            }
            None => {
                resp.expect_err("drain should fail");
            }
        }
    }
}
