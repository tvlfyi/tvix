//! This holds types describing nodes in the tvix-castore model.
mod directory;
mod directory_node;
mod file_node;
mod symlink_node;

use bytes::Bytes;
pub use directory::Directory;
pub use directory_node::DirectoryNode;
pub use file_node::FileNode;
pub use symlink_node::SymlinkNode;

/// A Node is either a [DirectoryNode], [FileNode] or [SymlinkNode].
/// While a Node by itself may have any name, only those matching specific requirements
/// can can be added as entries to a [Directory] (see the documentation on [Directory] for details).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Directory(DirectoryNode),
    File(FileNode),
    Symlink(SymlinkNode),
}

impl Node {
    /// Returns the node with a new name.
    pub fn rename(self, name: Bytes) -> Self {
        match self {
            Node::Directory(n) => Node::Directory(n.rename(name)),
            Node::File(n) => Node::File(n.rename(name)),
            Node::Symlink(n) => Node::Symlink(n.rename(name)),
        }
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

/// NamedNode is implemented for [FileNode], [DirectoryNode] and [SymlinkNode]
/// and [Node], so we can ask all of them for the name easily.
pub trait NamedNode {
    fn get_name(&self) -> &Bytes;
}

impl NamedNode for &Node {
    fn get_name(&self) -> &Bytes {
        match self {
            Node::File(node_file) => node_file.get_name(),
            Node::Directory(node_directory) => node_directory.get_name(),
            Node::Symlink(node_symlink) => node_symlink.get_name(),
        }
    }
}
impl NamedNode for Node {
    fn get_name(&self) -> &Bytes {
        match self {
            Node::File(node_file) => node_file.get_name(),
            Node::Directory(node_directory) => node_directory.get_name(),
            Node::Symlink(node_symlink) => node_symlink.get_name(),
        }
    }
}
