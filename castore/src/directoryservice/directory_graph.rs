use std::collections::HashMap;

use bstr::ByteSlice;

use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::{Bfs, DfsPostOrder, EdgeRef, IntoNodeIdentifiers, Walker},
    Direction, Incoming,
};
use tracing::instrument;

use super::order_validator::{LeavesToRootValidator, OrderValidator, RootToLeavesValidator};
use super::{Directory, DirectoryNode};
use crate::B3Digest;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    ValidationError(String),
}

/// This can be used to validate and/or re-order a Directory closure (DAG of
/// connected Directories), and their insertion order.
///
/// The DirectoryGraph is parametrized on the insertion order, and can be
/// constructed using the Default trait, or using `with_order` if the
/// OrderValidator needs to be customized.
///
/// If the user is receiving directories from canonical protobuf encoding in
/// root-to-leaves order, and parsing them, she can call `digest_allowed`
/// _before_ parsing the protobuf record and then add it with `add_unchecked`.
/// All other users insert the directories via `add`, in their specified order.
/// During insertion, we validate as much as we can at that time:
///
///  - individual validation of Directory messages
///  - validation of insertion order
///  - validation of size fields of referred Directories
///
/// Internally it keeps all received Directories in a directed graph,
/// with node weights being the Directories and edges pointing to child/parent
/// directories.
///
/// Once all Directories have been inserted, a validate function can be
/// called to perform a check for graph connectivity and ensure there's no
/// disconnected components or missing nodes.
/// Finally, the `drain_leaves_to_root` or `drain_root_to_leaves` can be
/// _chained_ on validate to get an iterator over the (deduplicated and)
/// validated list of directories in either order.
#[derive(Default)]
pub struct DirectoryGraph<O> {
    // A directed graph, using Directory as node weight.
    // Edges point from parents to children.
    //
    // Nodes with None weigths might exist when a digest has been referred to but the directory
    // with this digest has not yet been sent.
    //
    // The option in the edge weight tracks the pending validation state of the respective edge, for example if
    // the child has not been added yet.
    graph: DiGraph<Option<Directory>, Option<DirectoryNode>>,

    // A lookup table from directory digest to node index.
    digest_to_node_ix: HashMap<B3Digest, NodeIndex>,

    order_validator: O,
}

pub struct ValidatedDirectoryGraph {
    graph: DiGraph<Option<Directory>, Option<DirectoryNode>>,

    root: Option<NodeIndex>,
}

fn check_edge(dir: &DirectoryNode, child: &Directory) -> Result<(), Error> {
    // Ensure the size specified in the child node matches our records.
    if dir.size != child.size() {
        return Err(Error::ValidationError(format!(
            "'{}' has wrong size, specified {}, recorded {}",
            dir.name.as_bstr(),
            dir.size,
            child.size(),
        )));
    }
    Ok(())
}

impl DirectoryGraph<LeavesToRootValidator> {
    /// Insert a new Directory into the closure
    #[instrument(level = "trace", skip_all, fields(directory.digest=%directory.digest(), directory.size=%directory.size()), err)]
    pub fn add(&mut self, directory: Directory) -> Result<(), Error> {
        if !self.order_validator.add_directory(&directory) {
            return Err(Error::ValidationError(
                "unknown directory was referenced".into(),
            ));
        }
        self.add_order_unchecked(directory)
    }
}

impl DirectoryGraph<RootToLeavesValidator> {
    /// If the user is parsing directories from canonical protobuf encoding, she can
    /// call `digest_allowed` _before_ parsing the protobuf record and then add it
    /// with `add_unchecked`.
    pub fn digest_allowed(&self, digest: B3Digest) -> bool {
        self.order_validator.digest_allowed(&digest)
    }

    /// Insert a new Directory into the closure
    #[instrument(level = "trace", skip_all, fields(directory.digest=%directory.digest(), directory.size=%directory.size()), err)]
    pub fn add(&mut self, directory: Directory) -> Result<(), Error> {
        let digest = directory.digest();
        if !self.order_validator.digest_allowed(&digest) {
            return Err(Error::ValidationError("unexpected digest".into()));
        }
        self.order_validator.add_directory_unchecked(&directory);
        self.add_order_unchecked(directory)
    }
}

impl<O: OrderValidator> DirectoryGraph<O> {
    /// Customize the ordering, i.e. for pre-setting the root of the RootToLeavesValidator
    pub fn with_order(order_validator: O) -> Self {
        Self {
            graph: Default::default(),
            digest_to_node_ix: Default::default(),
            order_validator,
        }
    }

    /// Adds a directory which has already been confirmed to be in-order to the graph
    pub fn add_order_unchecked(&mut self, directory: Directory) -> Result<(), Error> {
        let digest = directory.digest();

        // Teach the graph about the existence of a node with this digest
        let ix = *self
            .digest_to_node_ix
            .entry(digest)
            .or_insert_with(|| self.graph.add_node(None));

        if self.graph[ix].is_some() {
            // The node is already in the graph, there is nothing to do here.
            return Ok(());
        }

        // set up edges to all child directories
        for subdir in directory.directories() {
            let child_ix = *self
                .digest_to_node_ix
                .entry(subdir.digest.clone())
                .or_insert_with(|| self.graph.add_node(None));

            let pending_edge_check = match &self.graph[child_ix] {
                Some(child) => {
                    // child is already available, validate the edge now
                    check_edge(subdir, child)?;
                    None
                }
                None => Some(subdir.clone()), // pending validation
            };
            self.graph.add_edge(ix, child_ix, pending_edge_check);
        }

        // validate the edges from parents to this node
        // this collects edge ids in a Vec because there is no edges_directed_mut :'c
        for edge_id in self
            .graph
            .edges_directed(ix, Direction::Incoming)
            .map(|edge_ref| edge_ref.id())
            .collect::<Vec<_>>()
            .into_iter()
        {
            let edge_weight = self
                .graph
                .edge_weight_mut(edge_id)
                .expect("edge not found")
                .take()
                .expect("edge is already validated");
            check_edge(&edge_weight, &directory)?;
        }

        // finally, store the directory information in the node weight
        self.graph[ix] = Some(directory);

        Ok(())
    }

    #[instrument(level = "trace", skip_all, err)]
    pub fn validate(self) -> Result<ValidatedDirectoryGraph, Error> {
        // find all initial nodes (nodes without incoming edges)
        let mut roots = self
            .graph
            .node_identifiers()
            .filter(|&a| self.graph.neighbors_directed(a, Incoming).next().is_none());

        let root = roots.next();
        if roots.next().is_some() {
            return Err(Error::ValidationError(
                "graph has disconnected roots".into(),
            ));
        }

        // test that the graph is complete
        if self.graph.raw_nodes().iter().any(|n| n.weight.is_none()) {
            return Err(Error::ValidationError("graph is incomplete".into()));
        }

        Ok(ValidatedDirectoryGraph {
            graph: self.graph,
            root,
        })
    }
}

impl ValidatedDirectoryGraph {
    /// Return the list of directories in from-root-to-leaves order.
    /// In case no elements have been inserted, returns an empty list.
    ///
    /// panics if the specified root is not in the graph
    #[instrument(level = "trace", skip_all)]
    pub fn drain_root_to_leaves(self) -> impl Iterator<Item = Directory> {
        let order = match self.root {
            Some(root) => {
                // do a BFS traversal of the graph, starting with the root node
                Bfs::new(&self.graph, root)
                    .iter(&self.graph)
                    .collect::<Vec<_>>()
            }
            None => vec![], // No nodes have been inserted, do not traverse
        };

        let (mut nodes, _edges) = self.graph.into_nodes_edges();

        order
            .into_iter()
            .filter_map(move |i| nodes[i.index()].weight.take())
    }

    /// Return the list of directories in from-leaves-to-root order.
    /// In case no elements have been inserted, returns an empty list.
    ///
    /// panics when the specified root is not in the graph
    #[instrument(level = "trace", skip_all)]
    pub fn drain_leaves_to_root(self) -> impl Iterator<Item = Directory> {
        let order = match self.root {
            Some(root) => {
                // do a DFS Post-Order traversal of the graph, starting with the root node
                DfsPostOrder::new(&self.graph, root)
                    .iter(&self.graph)
                    .collect::<Vec<_>>()
            }
            None => vec![], // No nodes have been inserted, do not traverse
        };

        let (mut nodes, _edges) = self.graph.into_nodes_edges();

        order
            .into_iter()
            .filter_map(move |i| nodes[i.index()].weight.take())
    }
}
/*
        pub static ref BROKEN_DIRECTORY : Directory = Directory {
            symlinks: vec![SymlinkNode {
                name: "".into(), // invalid name!
                target: "doesntmatter".into(),
            }],
            ..Default::default()
        };
*/
#[cfg(test)]
mod tests {
    use crate::directoryservice::{Directory, DirectoryNode, Node};
    use crate::fixtures::{DIRECTORY_A, DIRECTORY_B, DIRECTORY_C};
    use lazy_static::lazy_static;
    use rstest::rstest;

    use super::{DirectoryGraph, LeavesToRootValidator, RootToLeavesValidator};

    lazy_static! {
        pub static ref BROKEN_PARENT_DIRECTORY: Directory = {
            let mut dir = Directory::new();
            dir.add(Node::Directory(DirectoryNode::new(
                "foo".into(),
                DIRECTORY_A.digest(),
                DIRECTORY_A.size() + 42, // wrong!
            ).unwrap())).unwrap();
            dir
        };
    }

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
    /// Uploading a directory which refers to another Directory with a wrong size should fail.
    #[case::wrong_size_in_parent(&[&*DIRECTORY_A, &*BROKEN_PARENT_DIRECTORY], true, None)]
    fn test_uploads(
        #[case] directories_to_upload: &[&Directory],
        #[case] exp_fail_upload_last: bool,
        #[case] exp_finalize: Option<Vec<&Directory>>, // Some(_) if finalize successful, None if not.
    ) {
        let mut dcv = DirectoryGraph::<LeavesToRootValidator>::default();
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
        let resp = dcv
            .validate()
            .map(|validated| validated.drain_leaves_to_root().collect::<Vec<_>>());

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

    #[rstest]
    /// Downloading an empty directory should succeed.
    #[case::empty_directory(&*DIRECTORY_A, &[&*DIRECTORY_A], false, Some(vec![&*DIRECTORY_A]))]
    /// Downlading B, then A (referenced by B) should succeed.
    #[case::simple_closure(&*DIRECTORY_B, &[&*DIRECTORY_B, &*DIRECTORY_A], false, Some(vec![&*DIRECTORY_A, &*DIRECTORY_B]))]
    /// Downloading C (referring to A twice), then A should succeed.
    #[case::same_child_dedup(&*DIRECTORY_C, &[&*DIRECTORY_C, &*DIRECTORY_A], false, Some(vec![&*DIRECTORY_A, &*DIRECTORY_C]))]
    /// Downloading C, then B (both referring to A but not referring to each other) should fail immediately as B has no connection to C (the root)
    #[case::unconnected_node(&*DIRECTORY_C, &[&*DIRECTORY_C, &*DIRECTORY_B], true, None)]
    /// Downloading B (specified as the root) but receiving A instead should fail immediately, because A has no connection to B (the root).
    #[case::dangling_pointer(&*DIRECTORY_B, &[&*DIRECTORY_A], true, None)]
    /// Downloading a directory which refers to another Directory with a wrong size should fail.
    #[case::wrong_size_in_parent(&*BROKEN_PARENT_DIRECTORY, &[&*BROKEN_PARENT_DIRECTORY, &*DIRECTORY_A], true, None)]
    fn test_downloads(
        #[case] root: &Directory,
        #[case] directories_to_upload: &[&Directory],
        #[case] exp_fail_upload_last: bool,
        #[case] exp_finalize: Option<Vec<&Directory>>, // Some(_) if finalize successful, None if not.
    ) {
        let mut dcv =
            DirectoryGraph::with_order(RootToLeavesValidator::new_with_root_digest(root.digest()));
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
        let resp = dcv
            .validate()
            .map(|validated| validated.drain_leaves_to_root().collect::<Vec<_>>());

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
