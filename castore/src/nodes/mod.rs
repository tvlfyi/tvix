//! This holds types describing nodes in the tvix-castore model.
mod directory;
mod directory_node;
mod file_node;
mod symlink_node;

pub use directory::Directory;
pub use directory_node::DirectoryNode;
pub use file_node::FileNode;
pub use symlink_node::SymlinkNode;

/// A Node is either a [DirectoryNode], [FileNode] or [SymlinkNode].
/// Nodes themselves don't have names, what gives them names is either them
/// being inside a [Directory], or a root node with its own name attached to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Directory(DirectoryNode),
    File(FileNode),
    Symlink(SymlinkNode),
}
