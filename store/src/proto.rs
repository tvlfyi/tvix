#![allow(clippy::derive_partial_eq_without_eq)]
// https://github.com/hyperium/tonic/issues/1056
use std::collections::HashSet;
use thiserror::Error;

use prost::Message;

use nix_compat::store_path::{ParseStorePathError, StorePath};

tonic::include_proto!("tvix.store.v1");

#[cfg(feature = "reflection")]
/// Compiled file descriptors for implementing [gRPC
/// reflection](https://github.com/grpc/grpc/blob/master/doc/server-reflection.md) with e.g.
/// [`tonic_reflection`](https://docs.rs/tonic-reflection).
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("tvix.store.v1");

/// Errors that can occur during the validation of Directory messages.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum ValidateDirectoryError {
    /// Elements are not in sorted order
    #[error("{0} is not sorted")]
    WrongSorting(String),
    /// Multiple elements with the same name encountered
    #[error("{0} is a duplicate name")]
    DuplicateName(String),
    /// Invalid name encountered
    #[error("Invalid name in {0}")]
    InvalidName(String),
    /// Invalid digest length encountered
    #[error("Invalid Digest length: {0}")]
    InvalidDigestLen(usize),
}

/// Errors that can occur during the validation of PathInfo messages.
#[derive(Debug, Error, PartialEq)]
pub enum ValidatePathInfoError {
    /// No node present
    #[error("No node present")]
    NoNodePresent(),

    /// Invalid node name encountered.
    #[error("Failed to parse {0} as NixPath: {1}")]
    InvalidNodeName(String, ParseStorePathError),

    /// The digest the (root) node refers to has invalid length.
    #[error("Invalid Digest length: {0}")]
    InvalidDigestLen(usize),

    /// The number of references in the narinfo.reference_names field does not match
    /// the number of references in the .references field.
    #[error("Inconsistent Number of References: {0} (references) vs {0} (narinfo)")]
    InconsistentNumberOfReferences(usize, usize),
}

/// Checks a Node name for validity as an intermediate node, and returns an
/// error that's generated from the supplied constructor.
///
/// We disallow slashes, null bytes, '.', '..' and the empty string.
fn validate_node_name<E>(name: &str, err: fn(String) -> E) -> Result<(), E> {
    if name.is_empty() || name == ".." || name == "." || name.contains('\x00') || name.contains('/')
    {
        return Err(err(name.to_string()));
    }
    Ok(())
}

/// Checks a digest for validity.
/// Digests are 32 bytes long, as we store blake3 digests.
fn validate_digest<E>(digest: &Vec<u8>, err: fn(usize) -> E) -> Result<(), E> {
    if digest.len() != 32 {
        return Err(err(digest.len()));
    }
    Ok(())
}

/// Parses a root node name.
///
/// On success, this returns the parsed [StorePath].
/// On error, it returns an error generated from the supplied constructor.
fn parse_node_name_root<E>(
    name: &str,
    err: fn(String, ParseStorePathError) -> E,
) -> Result<StorePath, E> {
    match StorePath::from_string(name) {
        Ok(np) => Ok(np),
        Err(e) => Err(err(name.to_string(), e)),
    }
}

impl PathInfo {
    /// validate performs some checks on the PathInfo struct,
    /// Returning either a [StorePath] of the root node, or a
    /// [ValidatePathInfoError].
    pub fn validate(&self) -> Result<StorePath, ValidatePathInfoError> {
        // If there is a narinfo field populated, ensure the number of references there
        // matches PathInfo.references count.
        if let Some(narinfo) = &self.narinfo {
            if narinfo.reference_names.len() != self.references.len() {
                return Err(ValidatePathInfoError::InconsistentNumberOfReferences(
                    narinfo.reference_names.len(),
                    self.references.len(),
                ));
            }
        }
        // FUTUREWORK: parse references in reference_names. ensure they start
        // with storeDir, and use the same digest as in self.references.

        // Ensure there is a (root) node present, and it properly parses to a [StorePath].
        let root_nix_path = match &self.node {
            None => {
                return Err(ValidatePathInfoError::NoNodePresent());
            }
            Some(Node { node }) => match node {
                None => {
                    return Err(ValidatePathInfoError::NoNodePresent());
                }
                Some(node::Node::Directory(directory_node)) => {
                    // ensure the digest has the appropriate size.
                    validate_digest(
                        &directory_node.digest,
                        ValidatePathInfoError::InvalidDigestLen,
                    )?;

                    // parse the name
                    parse_node_name_root(
                        &directory_node.name,
                        ValidatePathInfoError::InvalidNodeName,
                    )?
                }
                Some(node::Node::File(file_node)) => {
                    // ensure the digest has the appropriate size.
                    validate_digest(&file_node.digest, ValidatePathInfoError::InvalidDigestLen)?;

                    // parse the name
                    parse_node_name_root(&file_node.name, ValidatePathInfoError::InvalidNodeName)?
                }
                Some(node::Node::Symlink(symlink_node)) => {
                    // parse the name
                    parse_node_name_root(
                        &symlink_node.name,
                        ValidatePathInfoError::InvalidNodeName,
                    )?
                }
            },
        };

        // return the root nix path
        Ok(root_nix_path)
    }
}

/// NamedNode is implemented for [FileNode], [DirectoryNode] and [SymlinkNode]
/// and [node::Node], so we can ask all of them for the name easily.
pub trait NamedNode {
    fn get_name(&self) -> &str;
}

impl NamedNode for &FileNode {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }
}

impl NamedNode for &DirectoryNode {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }
}

impl NamedNode for &SymlinkNode {
    fn get_name(&self) -> &str {
        self.name.as_str()
    }
}

impl NamedNode for node::Node {
    fn get_name(&self) -> &str {
        match self {
            node::Node::File(node_file) => &node_file.name,
            node::Node::Directory(node_directory) => &node_directory.name,
            node::Node::Symlink(node_symlink) => &node_symlink.name,
        }
    }
}

/// Accepts a name, and a mutable reference to the previous name.
/// If the passed name is larger than the previous one, the reference is updated.
/// If it's not, an error is returned.
fn update_if_lt_prev<'set, 'n>(
    prev_name: &'set mut &'n str,
    name: &'n str,
) -> Result<(), ValidateDirectoryError> {
    if *name < **prev_name {
        return Err(ValidateDirectoryError::WrongSorting(name.to_string()));
    }
    *prev_name = name;
    Ok(())
}

/// Inserts the given name into a HashSet if it's not already in there.
/// If it is, an error is returned.
fn insert_once<'n>(
    seen_names: &mut HashSet<&'n str>,
    name: &'n str,
) -> Result<(), ValidateDirectoryError> {
    if seen_names.get(name).is_some() {
        return Err(ValidateDirectoryError::DuplicateName(name.to_string()));
    }
    seen_names.insert(name);
    Ok(())
}

impl Directory {
    /// The size of a directory is the number of all regular and symlink elements,
    /// the number of directory elements, and their size fields.
    pub fn size(&self) -> u32 {
        self.files.len() as u32
            + self.symlinks.len() as u32
            + self
                .directories
                .iter()
                .fold(0, |acc: u32, e| (acc + 1 + e.size) as u32)
    }

    /// Calculates the digest of a Directory, which is the blake3 hash of a
    /// Directory protobuf message, serialized in protobuf canonical form.
    pub fn digest(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();

        hasher.update(&self.encode_to_vec()).finalize().as_bytes()[..].to_vec()
    }

    /// validate checks the directory for invalid data, such as:
    /// - violations of name restrictions
    /// - invalid digest lengths
    /// - not properly sorted lists
    /// - duplicate names in the three lists
    pub fn validate(&self) -> Result<(), ValidateDirectoryError> {
        let mut seen_names: HashSet<&str> = HashSet::new();

        let mut last_directory_name: &str = "";
        let mut last_file_name: &str = "";
        let mut last_symlink_name: &str = "";

        // check directories
        for directory_node in &self.directories {
            validate_node_name(&directory_node.name, ValidateDirectoryError::InvalidName)?;
            validate_digest(
                &directory_node.digest,
                ValidateDirectoryError::InvalidDigestLen,
            )?;

            update_if_lt_prev(&mut last_directory_name, directory_node.name.as_str())?;
            insert_once(&mut seen_names, directory_node.name.as_str())?;
        }

        // check files
        for file_node in &self.files {
            validate_node_name(&file_node.name, ValidateDirectoryError::InvalidName)?;
            validate_digest(&file_node.digest, ValidateDirectoryError::InvalidDigestLen)?;

            update_if_lt_prev(&mut last_file_name, file_node.name.as_str())?;
            insert_once(&mut seen_names, file_node.name.as_str())?;
        }

        // check symlinks
        for symlink_node in &self.symlinks {
            validate_node_name(&symlink_node.name, ValidateDirectoryError::InvalidName)?;

            update_if_lt_prev(&mut last_symlink_name, symlink_node.name.as_str())?;
            insert_once(&mut seen_names, symlink_node.name.as_str())?;
        }

        Ok(())
    }
}
