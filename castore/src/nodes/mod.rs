//! This holds types describing nodes in the tvix-castore model.
mod directory;
mod symlink_target;

use crate::B3Digest;
pub use directory::Directory;
pub use symlink_target::SymlinkTarget;

/// A Node is either a [DirectoryNode], [FileNode] or [SymlinkNode].
/// Nodes themselves don't have names, what gives them names is either them
/// being inside a [Directory], or a root node with its own name attached to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A DirectoryNode is a pointer to a [Directory], by its [Directory::digest].
    /// It also records a`size`.
    /// Such a node is either an element in the [Directory] it itself is contained in,
    /// or a standalone root node.
    Directory {
        /// The blake3 hash of a Directory message, serialized in protobuf canonical form.
        digest: B3Digest,
        /// Number of child elements in the Directory referred to by `digest`.
        /// Calculated by summing up the numbers of nodes, and for each directory,
        /// its size field. Can be used for inode allocation.
        /// This field is precisely as verifiable as any other Merkle tree edge.
        /// Resolve `digest`, and you can compute it incrementally. Resolve the entire
        /// tree, and you can fully compute it from scratch.
        /// A credulous implementation won't reject an excessive size, but this is
        /// harmless: you'll have some ordinals without nodes. Undersizing is obvious
        /// and easy to reject: you won't have an ordinal for some nodes.
        size: u64,
    },
    /// A FileNode represents a regular or executable file in a Directory or at the root.
    File {
        /// The blake3 digest of the file contents
        digest: B3Digest,

        /// The file content size
        size: u64,

        /// Whether the file is executable
        executable: bool,
    },
    /// A SymlinkNode represents a symbolic link in a Directory or at the root.
    Symlink {
        /// The target of the symlink.
        target: SymlinkTarget,
    },
}
