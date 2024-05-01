#![allow(non_snake_case)]
// https://github.com/hyperium/tonic/issues/1056
use bstr::ByteSlice;
use std::{collections::HashSet, iter::Peekable, str};

use prost::Message;

mod grpc_blobservice_wrapper;
mod grpc_directoryservice_wrapper;

pub use grpc_blobservice_wrapper::GRPCBlobServiceWrapper;
pub use grpc_directoryservice_wrapper::GRPCDirectoryServiceWrapper;

use crate::{B3Digest, B3_LEN};

tonic::include_proto!("tvix.castore.v1");

#[cfg(feature = "tonic-reflection")]
/// Compiled file descriptors for implementing [gRPC
/// reflection](https://github.com/grpc/grpc/blob/master/doc/server-reflection.md) with e.g.
/// [`tonic_reflection`](https://docs.rs/tonic-reflection).
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("tvix.castore.v1");

#[cfg(test)]
mod tests;

/// Errors that can occur during the validation of [Directory] messages.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidateDirectoryError {
    /// Elements are not in sorted order
    #[error("{:?} is not sorted", .0.as_bstr())]
    WrongSorting(Vec<u8>),
    /// Multiple elements with the same name encountered
    #[error("{:?} is a duplicate name", .0.as_bstr())]
    DuplicateName(Vec<u8>),
    /// Invalid node
    #[error("invalid node with name {:?}: {:?}", .0.as_bstr(), .1.to_string())]
    InvalidNode(Vec<u8>, ValidateNodeError),
    #[error("Total size exceeds u32::MAX")]
    SizeOverflow,
}

/// Errors that occur during Node validation
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidateNodeError {
    #[error("No node set")]
    NoNodeSet,
    /// Invalid digest length encountered
    #[error("Invalid Digest length: {0}")]
    InvalidDigestLen(usize),
    /// Invalid name encountered
    #[error("Invalid name: {}", .0.as_bstr())]
    InvalidName(Vec<u8>),
    /// Invalid symlink target
    #[error("Invalid symlink target: {}", .0.as_bstr())]
    InvalidSymlinkTarget(Vec<u8>),
}

/// Errors that occur during StatBlobResponse validation
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidateStatBlobResponseError {
    /// Invalid digest length encountered
    #[error("Invalid digest length {0} for chunk #{1}")]
    InvalidDigestLen(usize, usize),
}

/// Checks a Node name for validity as an intermediate node.
/// We disallow slashes, null bytes, '.', '..' and the empty string.
pub(crate) fn validate_node_name(name: &[u8]) -> Result<(), ValidateNodeError> {
    if name.is_empty()
        || name == b".."
        || name == b"."
        || name.contains(&0x00)
        || name.contains(&b'/')
    {
        Err(ValidateNodeError::InvalidName(name.to_owned()))
    } else {
        Ok(())
    }
}

/// NamedNode is implemented for [FileNode], [DirectoryNode] and [SymlinkNode]
/// and [node::Node], so we can ask all of them for the name easily.
pub trait NamedNode {
    fn get_name(&self) -> &[u8];
}

impl NamedNode for &FileNode {
    fn get_name(&self) -> &[u8] {
        &self.name
    }
}

impl NamedNode for &DirectoryNode {
    fn get_name(&self) -> &[u8] {
        &self.name
    }
}

impl NamedNode for &SymlinkNode {
    fn get_name(&self) -> &[u8] {
        &self.name
    }
}

impl NamedNode for node::Node {
    fn get_name(&self) -> &[u8] {
        match self {
            node::Node::File(node_file) => &node_file.name,
            node::Node::Directory(node_directory) => &node_directory.name,
            node::Node::Symlink(node_symlink) => &node_symlink.name,
        }
    }
}

impl Node {
    /// Ensures the node has a valid enum kind (is Some), and passes its
    // per-enum validation.
    pub fn validate(&self) -> Result<(), ValidateNodeError> {
        if let Some(node) = self.node.as_ref() {
            node.validate()
        } else {
            Err(ValidateNodeError::NoNodeSet)
        }
    }
}

impl node::Node {
    /// Returns the node with a new name.
    pub fn rename(self, name: bytes::Bytes) -> Self {
        match self {
            node::Node::Directory(n) => node::Node::Directory(DirectoryNode { name, ..n }),
            node::Node::File(n) => node::Node::File(FileNode { name, ..n }),
            node::Node::Symlink(n) => node::Node::Symlink(SymlinkNode { name, ..n }),
        }
    }

    /// Ensures the node has a valid name, and checks the type-specific fields too.
    pub fn validate(&self) -> Result<(), ValidateNodeError> {
        match self {
            // for a directory root node, ensure the digest has the appropriate size.
            node::Node::Directory(directory_node) => {
                if directory_node.digest.len() != B3_LEN {
                    Err(ValidateNodeError::InvalidDigestLen(
                        directory_node.digest.len(),
                    ))?;
                }
                validate_node_name(&directory_node.name)
            }
            // for a file root node, ensure the digest has the appropriate size.
            node::Node::File(file_node) => {
                if file_node.digest.len() != B3_LEN {
                    Err(ValidateNodeError::InvalidDigestLen(file_node.digest.len()))?;
                }
                validate_node_name(&file_node.name)
            }
            // ensure the symlink target is not empty and doesn't contain null bytes.
            node::Node::Symlink(symlink_node) => {
                if symlink_node.target.is_empty() || symlink_node.target.contains(&b'\0') {
                    Err(ValidateNodeError::InvalidSymlinkTarget(
                        symlink_node.target.to_vec(),
                    ))?;
                }
                validate_node_name(&symlink_node.name)
            }
        }
    }
}

impl PartialOrd for node::Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for node::Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl PartialOrd for FileNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl PartialOrd for SymlinkNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SymlinkNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl PartialOrd for DirectoryNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DirectoryNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

/// Accepts a name, and a mutable reference to the previous name.
/// If the passed name is larger than the previous one, the reference is updated.
/// If it's not, an error is returned.
fn update_if_lt_prev<'n>(
    prev_name: &mut &'n [u8],
    name: &'n [u8],
) -> Result<(), ValidateDirectoryError> {
    if *name < **prev_name {
        return Err(ValidateDirectoryError::WrongSorting(name.to_vec()));
    }
    *prev_name = name;
    Ok(())
}

/// Inserts the given name into a HashSet if it's not already in there.
/// If it is, an error is returned.
fn insert_once<'n>(
    seen_names: &mut HashSet<&'n [u8]>,
    name: &'n [u8],
) -> Result<(), ValidateDirectoryError> {
    if seen_names.get(name).is_some() {
        return Err(ValidateDirectoryError::DuplicateName(name.to_vec()));
    }
    seen_names.insert(name);
    Ok(())
}

fn checked_sum(iter: impl IntoIterator<Item = u64>) -> Option<u64> {
    iter.into_iter().try_fold(0u64, |acc, i| acc.checked_add(i))
}

impl Directory {
    /// The size of a directory is the number of all regular and symlink elements,
    /// the number of directory elements, and their size fields.
    pub fn size(&self) -> u64 {
        if cfg!(debug_assertions) {
            self.size_checked()
                .expect("Directory::size exceeds u64::MAX")
        } else {
            self.size_checked().unwrap_or(u64::MAX)
        }
    }

    fn size_checked(&self) -> Option<u64> {
        checked_sum([
            self.files.len().try_into().ok()?,
            self.symlinks.len().try_into().ok()?,
            self.directories.len().try_into().ok()?,
            checked_sum(self.directories.iter().map(|e| e.size))?,
        ])
    }

    /// Calculates the digest of a Directory, which is the blake3 hash of a
    /// Directory protobuf message, serialized in protobuf canonical form.
    pub fn digest(&self) -> B3Digest {
        let mut hasher = blake3::Hasher::new();

        hasher
            .update(&self.encode_to_vec())
            .finalize()
            .as_bytes()
            .into()
    }

    /// validate checks the directory for invalid data, such as:
    /// - violations of name restrictions
    /// - invalid digest lengths
    /// - not properly sorted lists
    /// - duplicate names in the three lists
    pub fn validate(&self) -> Result<(), ValidateDirectoryError> {
        let mut seen_names: HashSet<&[u8]> = HashSet::new();

        let mut last_directory_name: &[u8] = b"";
        let mut last_file_name: &[u8] = b"";
        let mut last_symlink_name: &[u8] = b"";

        // check directories
        for directory_node in &self.directories {
            node::Node::Directory(directory_node.clone())
                .validate()
                .map_err(|e| {
                    ValidateDirectoryError::InvalidNode(directory_node.name.to_vec(), e)
                })?;

            update_if_lt_prev(&mut last_directory_name, &directory_node.name)?;
            insert_once(&mut seen_names, &directory_node.name)?;
        }

        // check files
        for file_node in &self.files {
            node::Node::File(file_node.clone())
                .validate()
                .map_err(|e| ValidateDirectoryError::InvalidNode(file_node.name.to_vec(), e))?;

            update_if_lt_prev(&mut last_file_name, &file_node.name)?;
            insert_once(&mut seen_names, &file_node.name)?;
        }

        // check symlinks
        for symlink_node in &self.symlinks {
            node::Node::Symlink(symlink_node.clone())
                .validate()
                .map_err(|e| ValidateDirectoryError::InvalidNode(symlink_node.name.to_vec(), e))?;

            update_if_lt_prev(&mut last_symlink_name, &symlink_node.name)?;
            insert_once(&mut seen_names, &symlink_node.name)?;
        }

        self.size_checked()
            .ok_or(ValidateDirectoryError::SizeOverflow)?;

        Ok(())
    }

    /// Allows iterating over all three nodes ([DirectoryNode], [FileNode],
    /// [SymlinkNode]) in an ordered fashion, as long as the individual lists
    /// are sorted (which can be checked by the [Directory::validate]).
    pub fn nodes(&self) -> DirectoryNodesIterator {
        return DirectoryNodesIterator {
            i_directories: self.directories.iter().peekable(),
            i_files: self.files.iter().peekable(),
            i_symlinks: self.symlinks.iter().peekable(),
        };
    }

    /// Adds the specified [node::Node] to the [Directory], preserving sorted entries.
    /// This assumes the [Directory] to be sorted prior to adding the node.
    ///
    /// Inserting an element that already exists with the same name in the directory is not
    /// supported.
    pub fn add(&mut self, node: node::Node) {
        debug_assert!(
            !self.files.iter().any(|x| x.get_name() == node.get_name()),
            "name already exists in files"
        );
        debug_assert!(
            !self
                .directories
                .iter()
                .any(|x| x.get_name() == node.get_name()),
            "name already exists in directories"
        );
        debug_assert!(
            !self
                .symlinks
                .iter()
                .any(|x| x.get_name() == node.get_name()),
            "name already exists in symlinks"
        );

        match node {
            node::Node::File(node) => {
                let pos = self
                    .files
                    .binary_search(&node)
                    .expect_err("Tvix bug: dir entry with name already exists");
                self.files.insert(pos, node);
            }
            node::Node::Directory(node) => {
                let pos = self
                    .directories
                    .binary_search(&node)
                    .expect_err("Tvix bug: dir entry with name already exists");
                self.directories.insert(pos, node);
            }
            node::Node::Symlink(node) => {
                let pos = self
                    .symlinks
                    .binary_search(&node)
                    .expect_err("Tvix bug: dir entry with name already exists");
                self.symlinks.insert(pos, node);
            }
        }
    }
}

impl StatBlobResponse {
    /// Validates a StatBlobResponse. All chunks must have valid blake3 digests.
    /// It is allowed to send an empty list, if no more granular chunking is
    /// available.
    pub fn validate(&self) -> Result<(), ValidateStatBlobResponseError> {
        for (i, chunk) in self.chunks.iter().enumerate() {
            if chunk.digest.len() != blake3::KEY_LEN {
                return Err(ValidateStatBlobResponseError::InvalidDigestLen(
                    chunk.digest.len(),
                    i,
                ));
            }
        }
        Ok(())
    }
}

/// Struct to hold the state of an iterator over all nodes of a Directory.
///
/// Internally, this keeps peekable Iterators over all three lists of a
/// Directory message.
pub struct DirectoryNodesIterator<'a> {
    // directory: &Directory,
    i_directories: Peekable<std::slice::Iter<'a, DirectoryNode>>,
    i_files: Peekable<std::slice::Iter<'a, FileNode>>,
    i_symlinks: Peekable<std::slice::Iter<'a, SymlinkNode>>,
}

/// looks at two elements implementing NamedNode, and returns true if "left
/// is smaller / comes first".
///
/// Some(_) is preferred over None.
fn left_name_lt_right<A: NamedNode, B: NamedNode>(left: Option<&A>, right: Option<&B>) -> bool {
    match left {
        // if left is None, right always wins
        None => false,
        Some(left_inner) => {
            // left is Some.
            match right {
                // left is Some, right is None - left wins.
                None => true,
                Some(right_inner) => {
                    // both are Some - compare the name.
                    return left_inner.get_name() < right_inner.get_name();
                }
            }
        }
    }
}

impl Iterator for DirectoryNodesIterator<'_> {
    type Item = node::Node;

    // next returns the next node in the Directory.
    // we peek at all three internal iterators, and pick the one with the
    // smallest name, to ensure lexicographical ordering.
    // The individual lists are already known to be sorted.
    fn next(&mut self) -> Option<Self::Item> {
        if left_name_lt_right(self.i_directories.peek(), self.i_files.peek()) {
            // i_directories is still in the game, compare with symlinks
            if left_name_lt_right(self.i_directories.peek(), self.i_symlinks.peek()) {
                self.i_directories
                    .next()
                    .cloned()
                    .map(node::Node::Directory)
            } else {
                self.i_symlinks.next().cloned().map(node::Node::Symlink)
            }
        } else {
            // i_files is still in the game, compare with symlinks
            if left_name_lt_right(self.i_files.peek(), self.i_symlinks.peek()) {
                self.i_files.next().cloned().map(node::Node::File)
            } else {
                self.i_symlinks.next().cloned().map(node::Node::Symlink)
            }
        }
    }
}
