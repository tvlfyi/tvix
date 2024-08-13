use std::str;

use prost::Message;

mod grpc_blobservice_wrapper;
mod grpc_directoryservice_wrapper;

pub use grpc_blobservice_wrapper::GRPCBlobServiceWrapper;
pub use grpc_directoryservice_wrapper::GRPCDirectoryServiceWrapper;

use crate::NamedNode;
use crate::{B3Digest, ValidateDirectoryError, ValidateNodeError};

tonic::include_proto!("tvix.castore.v1");

#[cfg(feature = "tonic-reflection")]
/// Compiled file descriptors for implementing [gRPC
/// reflection](https://github.com/grpc/grpc/blob/master/doc/server-reflection.md) with e.g.
/// [`tonic_reflection`](https://docs.rs/tonic-reflection).
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("tvix.castore.v1");

#[cfg(test)]
mod tests;

/// Errors that occur during StatBlobResponse validation
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ValidateStatBlobResponseError {
    /// Invalid digest length encountered
    #[error("Invalid digest length {0} for chunk #{1}")]
    InvalidDigestLen(usize, usize),
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

impl TryFrom<&node::Node> for crate::Node {
    type Error = ValidateNodeError;

    fn try_from(node: &node::Node) -> Result<crate::Node, ValidateNodeError> {
        Ok(match node {
            node::Node::Directory(n) => crate::Node::Directory(n.try_into()?),
            node::Node::File(n) => crate::Node::File(n.try_into()?),
            node::Node::Symlink(n) => crate::Node::Symlink(n.try_into()?),
        })
    }
}

impl TryFrom<&Node> for crate::Node {
    type Error = ValidateNodeError;

    fn try_from(node: &Node) -> Result<crate::Node, ValidateNodeError> {
        match node {
            Node { node: None } => Err(ValidateNodeError::NoNodeSet),
            Node { node: Some(node) } => node.try_into(),
        }
    }
}

impl TryFrom<&DirectoryNode> for crate::DirectoryNode {
    type Error = ValidateNodeError;

    fn try_from(node: &DirectoryNode) -> Result<crate::DirectoryNode, ValidateNodeError> {
        crate::DirectoryNode::new(
            node.name.clone(),
            node.digest.clone().try_into()?,
            node.size,
        )
    }
}

impl TryFrom<&SymlinkNode> for crate::SymlinkNode {
    type Error = ValidateNodeError;

    fn try_from(node: &SymlinkNode) -> Result<crate::SymlinkNode, ValidateNodeError> {
        crate::SymlinkNode::new(node.name.clone(), node.target.clone())
    }
}

impl TryFrom<&FileNode> for crate::FileNode {
    type Error = ValidateNodeError;

    fn try_from(node: &FileNode) -> Result<crate::FileNode, ValidateNodeError> {
        crate::FileNode::new(
            node.name.clone(),
            node.digest.clone().try_into()?,
            node.size,
            node.executable,
        )
    }
}

impl TryFrom<Directory> for crate::Directory {
    type Error = ValidateDirectoryError;

    fn try_from(directory: Directory) -> Result<crate::Directory, ValidateDirectoryError> {
        (&directory).try_into()
    }
}

impl TryFrom<&Directory> for crate::Directory {
    type Error = ValidateDirectoryError;

    fn try_from(directory: &Directory) -> Result<crate::Directory, ValidateDirectoryError> {
        let mut dir = crate::Directory::new();
        let mut last_file_name: &[u8] = b"";
        for file in directory.files.iter().map(move |file| {
            update_if_lt_prev(&mut last_file_name, &file.name).map(|()| file.clone())
        }) {
            let file = file?;
            dir.add(crate::Node::File((&file).try_into().map_err(|e| {
                ValidateDirectoryError::InvalidNode(file.name.into(), e)
            })?))?;
        }
        let mut last_directory_name: &[u8] = b"";
        for directory in directory.directories.iter().map(move |directory| {
            update_if_lt_prev(&mut last_directory_name, &directory.name).map(|()| directory.clone())
        }) {
            let directory = directory?;
            dir.add(crate::Node::Directory((&directory).try_into().map_err(
                |e| ValidateDirectoryError::InvalidNode(directory.name.into(), e),
            )?))?;
        }
        let mut last_symlink_name: &[u8] = b"";
        for symlink in directory.symlinks.iter().map(move |symlink| {
            update_if_lt_prev(&mut last_symlink_name, &symlink.name).map(|()| symlink.clone())
        }) {
            let symlink = symlink?;
            dir.add(crate::Node::Symlink((&symlink).try_into().map_err(
                |e| ValidateDirectoryError::InvalidNode(symlink.name.into(), e),
            )?))?;
        }
        Ok(dir)
    }
}

impl From<&crate::Node> for node::Node {
    fn from(node: &crate::Node) -> node::Node {
        match node {
            crate::Node::Directory(n) => node::Node::Directory(n.into()),
            crate::Node::File(n) => node::Node::File(n.into()),
            crate::Node::Symlink(n) => node::Node::Symlink(n.into()),
        }
    }
}

impl From<&crate::Node> for Node {
    fn from(node: &crate::Node) -> Node {
        Node {
            node: Some(node.into()),
        }
    }
}

impl From<&crate::DirectoryNode> for DirectoryNode {
    fn from(node: &crate::DirectoryNode) -> DirectoryNode {
        DirectoryNode {
            digest: node.digest().clone().into(),
            size: node.size(),
            name: node.get_name().clone(),
        }
    }
}

impl From<&crate::FileNode> for FileNode {
    fn from(node: &crate::FileNode) -> FileNode {
        FileNode {
            digest: node.digest().clone().into(),
            size: node.size(),
            name: node.get_name().clone(),
            executable: node.executable(),
        }
    }
}

impl From<&crate::SymlinkNode> for SymlinkNode {
    fn from(node: &crate::SymlinkNode) -> SymlinkNode {
        SymlinkNode {
            name: node.get_name().clone(),
            target: node.target().clone(),
        }
    }
}

impl From<crate::Directory> for Directory {
    fn from(directory: crate::Directory) -> Directory {
        (&directory).into()
    }
}

impl From<&crate::Directory> for Directory {
    fn from(directory: &crate::Directory) -> Directory {
        let mut directories = vec![];
        let mut files = vec![];
        let mut symlinks = vec![];
        for node in directory.nodes() {
            match node {
                crate::Node::File(n) => {
                    files.push(n.into());
                }
                crate::Node::Directory(n) => {
                    directories.push(n.into());
                }
                crate::Node::Symlink(n) => {
                    symlinks.push(n.into());
                }
            }
        }
        Directory {
            directories,
            files,
            symlinks,
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
