//! This module contains all the data structures used to track information
//! about inodes, which present tvix-castore nodes in a filesystem.
use std::time::Duration;

use crate::proto as castorepb;
use crate::B3Digest;

#[derive(Clone, Debug)]
pub enum InodeData {
    Regular(B3Digest, u64, bool),  // digest, size, executable
    Symlink(bytes::Bytes),         // target
    Directory(DirectoryInodeData), // either [DirectoryInodeData:Sparse] or [DirectoryInodeData:Populated]
}

impl InodeData {
    pub fn as_fuse_file_attr(&self, inode: u64) -> fuse_backend_rs::abi::fuse_abi::Attr {
        fuse_backend_rs::abi::fuse_abi::Attr {
            ino: inode,
            // FUTUREWORK: play with this numbers, as it affects read sizes for client applications.
            blocks: 1024,
            size: match self {
                InodeData::Regular(_, size, _) => *size,
                InodeData::Symlink(target) => target.len() as u64,
                InodeData::Directory(DirectoryInodeData::Sparse(_, size)) => *size,
                InodeData::Directory(DirectoryInodeData::Populated(_, ref children)) => {
                    children.len() as u64
                }
            },
            mode: match self {
                InodeData::Regular(_, _, false) => libc::S_IFREG | 0o444, // no-executable files
                InodeData::Regular(_, _, true) => libc::S_IFREG | 0o555,  // executable files
                InodeData::Symlink(_) => libc::S_IFLNK | 0o444,
                InodeData::Directory(_) => libc::S_IFDIR | 0o555,
            },
            ..Default::default()
        }
    }

    pub fn as_fuse_entry(&self, inode: u64) -> fuse_backend_rs::api::filesystem::Entry {
        fuse_backend_rs::api::filesystem::Entry {
            inode,
            attr: self.as_fuse_file_attr(inode).into(),
            attr_timeout: Duration::MAX,
            entry_timeout: Duration::MAX,
            ..Default::default()
        }
    }
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
