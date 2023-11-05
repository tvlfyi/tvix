//! This module contains all the data structures used to track information
//! about inodes, which present tvix-store nodes in a filesystem.
use tvix_castore::proto as castorepb;
use tvix_castore::B3Digest;

#[derive(Clone, Debug)]
pub enum InodeData {
    Regular(B3Digest, u64, bool),  // digest, size, executable
    Symlink(bytes::Bytes),         // target
    Directory(DirectoryInodeData), // either [DirectoryInodeData:Sparse] or [DirectoryInodeData:Populated]
}

/// This encodes the two different states of [InodeData::Directory].
/// Either the data still is sparse (we only saw a [castorepb::DirectoryNode],
/// but didn't fetch the [castorepb::Directory] struct yet, or we processed a
/// lookup and did fetch the data.
#[derive(Clone, Debug)]
pub enum DirectoryInodeData {
    Sparse(B3Digest, u64),                                  // digest, size
    Populated(B3Digest, Vec<(u64, castorepb::node::Node)>), // [(child_inode, node)]
}

impl From<&castorepb::node::Node> for InodeData {
    fn from(value: &castorepb::node::Node) -> Self {
        match value {
            castorepb::node::Node::Directory(directory_node) => directory_node.into(),
            castorepb::node::Node::File(file_node) => file_node.into(),
            castorepb::node::Node::Symlink(symlink_node) => symlink_node.into(),
        }
    }
}

impl From<&castorepb::SymlinkNode> for InodeData {
    fn from(value: &castorepb::SymlinkNode) -> Self {
        InodeData::Symlink(value.target.clone())
    }
}

impl From<&castorepb::FileNode> for InodeData {
    fn from(value: &castorepb::FileNode) -> Self {
        InodeData::Regular(
            value.digest.clone().try_into().unwrap(),
            value.size,
            value.executable,
        )
    }
}

/// Converts a DirectoryNode to a sparsely populated InodeData::Directory.
impl From<&castorepb::DirectoryNode> for InodeData {
    fn from(value: &castorepb::DirectoryNode) -> Self {
        InodeData::Directory(DirectoryInodeData::Sparse(
            value.digest.clone().try_into().unwrap(),
            value.size,
        ))
    }
}

/// converts a proto::Directory to a InodeData::Directory(DirectoryInodeData::Populated(..)).
/// The inodes for each child are 0, because it's up to the InodeTracker to allocate them.
impl From<castorepb::Directory> for InodeData {
    fn from(value: castorepb::Directory) -> Self {
        let digest = value.digest();

        let children: Vec<(u64, castorepb::node::Node)> =
            value.nodes().map(|node| (0, node)).collect();

        InodeData::Directory(DirectoryInodeData::Populated(digest, children))
    }
}
