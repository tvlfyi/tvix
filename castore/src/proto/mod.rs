use prost::Message;
use std::cmp::Ordering;

mod grpc_blobservice_wrapper;
mod grpc_directoryservice_wrapper;

use crate::{path::PathComponent, B3Digest, DirectoryError};
pub use grpc_blobservice_wrapper::GRPCBlobServiceWrapper;
pub use grpc_directoryservice_wrapper::GRPCDirectoryServiceWrapper;

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

impl TryFrom<Directory> for crate::Directory {
    type Error = DirectoryError;

    fn try_from(value: Directory) -> Result<Self, Self::Error> {
        // Check directories, files and symlinks are sorted
        // We'll notice duplicates across all three fields when constructing the Directory.
        // FUTUREWORK: use is_sorted() once stable, and/or implement the producer for
        // [crate::Directory::try_from_iter] iterating over all three and doing all checks inline.
        value
            .directories
            .iter()
            .try_fold(&b""[..], |prev_name, e| {
                match e.name.as_ref().cmp(prev_name) {
                    Ordering::Less => Err(DirectoryError::WrongSorting(e.name.to_owned())),
                    Ordering::Equal => Err(DirectoryError::DuplicateName(
                        e.name
                            .to_owned()
                            .try_into()
                            .map_err(DirectoryError::InvalidName)?,
                    )),
                    Ordering::Greater => Ok(e.name.as_ref()),
                }
            })?;
        value.files.iter().try_fold(&b""[..], |prev_name, e| {
            match e.name.as_ref().cmp(prev_name) {
                Ordering::Less => Err(DirectoryError::WrongSorting(e.name.to_owned())),
                Ordering::Equal => Err(DirectoryError::DuplicateName(
                    e.name
                        .to_owned()
                        .try_into()
                        .map_err(DirectoryError::InvalidName)?,
                )),
                Ordering::Greater => Ok(e.name.as_ref()),
            }
        })?;
        value.symlinks.iter().try_fold(&b""[..], |prev_name, e| {
            match e.name.as_ref().cmp(prev_name) {
                Ordering::Less => Err(DirectoryError::WrongSorting(e.name.to_owned())),
                Ordering::Equal => Err(DirectoryError::DuplicateName(
                    e.name
                        .to_owned()
                        .try_into()
                        .map_err(DirectoryError::InvalidName)?,
                )),
                Ordering::Greater => Ok(e.name.as_ref()),
            }
        })?;

        // FUTUREWORK: use is_sorted() once stable, and/or implement the producer for
        // [crate::Directory::try_from_iter] iterating over all three and doing all checks inline.
        let mut elems: Vec<(PathComponent, crate::Node)> =
            Vec::with_capacity(value.directories.len() + value.files.len() + value.symlinks.len());

        for e in value.directories {
            elems.push(
                Node {
                    node: Some(node::Node::Directory(e)),
                }
                .into_name_and_node()?,
            );
        }

        for e in value.files {
            elems.push(
                Node {
                    node: Some(node::Node::File(e)),
                }
                .into_name_and_node()?,
            )
        }

        for e in value.symlinks {
            elems.push(
                Node {
                    node: Some(node::Node::Symlink(e)),
                }
                .into_name_and_node()?,
            )
        }

        crate::Directory::try_from_iter(elems)
    }
}

impl From<crate::Directory> for Directory {
    fn from(value: crate::Directory) -> Self {
        let mut directories = vec![];
        let mut files = vec![];
        let mut symlinks = vec![];

        for (name, node) in value.into_nodes() {
            match node {
                crate::Node::File {
                    digest,
                    size,
                    executable,
                } => files.push(FileNode {
                    name: name.into(),
                    digest: digest.into(),
                    size,
                    executable,
                }),
                crate::Node::Directory { digest, size } => directories.push(DirectoryNode {
                    name: name.into(),
                    digest: digest.into(),
                    size,
                }),
                crate::Node::Symlink { target } => {
                    symlinks.push(SymlinkNode {
                        name: name.into(),
                        target: target.into(),
                    });
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

impl Node {
    /// Converts a proto [Node] to a [crate::Node], and splits off the name.
    pub fn into_name_and_node(self) -> Result<(PathComponent, crate::Node), DirectoryError> {
        match self.node.ok_or_else(|| DirectoryError::NoNodeSet)? {
            node::Node::Directory(n) => {
                let name: PathComponent = n.name.try_into().map_err(DirectoryError::InvalidName)?;
                let digest = B3Digest::try_from(n.digest)
                    .map_err(|e| DirectoryError::InvalidNode(name.clone(), e.into()))?;

                let node = crate::Node::Directory {
                    digest,
                    size: n.size,
                };

                Ok((name, node))
            }
            node::Node::File(n) => {
                let name: PathComponent = n.name.try_into().map_err(DirectoryError::InvalidName)?;
                let digest = B3Digest::try_from(n.digest)
                    .map_err(|e| DirectoryError::InvalidNode(name.clone(), e.into()))?;

                let node = crate::Node::File {
                    digest,
                    size: n.size,
                    executable: n.executable,
                };

                Ok((name, node))
            }

            node::Node::Symlink(n) => {
                let name: PathComponent = n.name.try_into().map_err(DirectoryError::InvalidName)?;

                let node = crate::Node::Symlink {
                    target: n.target.try_into().map_err(|e| {
                        DirectoryError::InvalidNode(
                            name.clone(),
                            crate::ValidateNodeError::InvalidSymlinkTarget(e),
                        )
                    })?,
                };

                Ok((name, node))
            }
        }
    }

    /// Construsts a [Node] from a name and [crate::Node].
    /// The name is a [bytes::Bytes], not a [PathComponent], as we have use an
    /// empty name in some places.
    pub fn from_name_and_node(name: bytes::Bytes, n: crate::Node) -> Self {
        match n {
            crate::Node::Directory { digest, size } => Self {
                node: Some(node::Node::Directory(DirectoryNode {
                    name,
                    digest: digest.into(),
                    size,
                })),
            },
            crate::Node::File {
                digest,
                size,
                executable,
            } => Self {
                node: Some(node::Node::File(FileNode {
                    name,
                    digest: digest.into(),
                    size,
                    executable,
                })),
            },
            crate::Node::Symlink { target } => Self {
                node: Some(node::Node::Symlink(SymlinkNode {
                    name,
                    target: target.into(),
                })),
            },
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
