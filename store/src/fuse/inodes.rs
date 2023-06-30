//! This module contains all the data structures used to track information
//! about inodes, which present tvix-store nodes in a filesystem.
use crate::{proto, B3Digest};

#[derive(Clone, Debug)]
pub enum InodeData {
    Regular(B3Digest, u32, bool),  // digest, size, executable
    Symlink(String),               // target
    Directory(DirectoryInodeData), // either [DirectoryInodeData:Sparse] or [DirectoryInodeData:Populated]
}

/// This encodes the two different states of [InodeData::Directory].
/// Either the data still is sparse (we only saw a [proto::DirectoryNode], but
/// didn't fetch the [proto::Directory] struct yet,
/// or we processed a lookup and did fetch the data.
#[derive(Clone, Debug)]
pub enum DirectoryInodeData {
    Sparse(B3Digest, u32),                              // digest, size
    Populated(B3Digest, Vec<(u64, proto::node::Node)>), // [(child_inode, node)]
}

impl From<&proto::node::Node> for InodeData {
    fn from(value: &proto::node::Node) -> Self {
        match value {
            proto::node::Node::Directory(directory_node) => directory_node.into(),
            proto::node::Node::File(file_node) => file_node.into(),
            proto::node::Node::Symlink(symlink_node) => symlink_node.into(),
        }
    }
}

impl From<&proto::SymlinkNode> for InodeData {
    fn from(value: &proto::SymlinkNode) -> Self {
        InodeData::Symlink(value.target.clone())
    }
}

impl From<&proto::FileNode> for InodeData {
    fn from(value: &proto::FileNode) -> Self {
        InodeData::Regular(
            B3Digest::from_vec(value.digest.clone()).unwrap(),
            value.size,
            value.executable,
        )
    }
}

/// Converts a DirectoryNode to a sparsely populated InodeData::Directory.
impl From<&proto::DirectoryNode> for InodeData {
    fn from(value: &proto::DirectoryNode) -> Self {
        InodeData::Directory(DirectoryInodeData::Sparse(
            B3Digest::from_vec(value.digest.clone()).unwrap(),
            value.size,
        ))
    }
}

/// converts a proto::Directory to a InodeData::Directory(DirectoryInodeData::Populated(..)).
/// The inodes for each child are 0, because it's up to the InodeTracker to allocate them.
impl From<proto::Directory> for InodeData {
    fn from(value: proto::Directory) -> Self {
        let digest = value.digest();

        let children: Vec<(u64, proto::node::Node)> = value.nodes().map(|node| (0, node)).collect();

        InodeData::Directory(DirectoryInodeData::Populated(digest, children))
    }
}

impl From<&InodeData> for fuser::FileType {
    fn from(val: &InodeData) -> Self {
        match val {
            InodeData::Regular(..) => fuser::FileType::RegularFile,
            InodeData::Symlink(_) => fuser::FileType::Symlink,
            InodeData::Directory(..) => fuser::FileType::Directory,
        }
    }
}
