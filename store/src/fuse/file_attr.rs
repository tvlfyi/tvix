use std::time::SystemTime;

use super::inodes::{DirectoryInodeData, InodeData};
use fuser::FileAttr;

/// The [FileAttr] describing the root
pub const ROOT_FILE_ATTR: FileAttr = FileAttr {
    ino: fuser::FUSE_ROOT_ID,
    size: 0,
    blksize: 1024,
    blocks: 0,
    atime: SystemTime::UNIX_EPOCH,
    mtime: SystemTime::UNIX_EPOCH,
    ctime: SystemTime::UNIX_EPOCH,
    crtime: SystemTime::UNIX_EPOCH,
    kind: fuser::FileType::Directory,
    perm: 0o555,
    nlink: 0,
    uid: 0,
    gid: 0,
    rdev: 0,
    flags: 0,
};

/// for given &Node and inode, construct a [FileAttr]
pub fn gen_file_attr(inode_data: &InodeData, inode: u64) -> FileAttr {
    FileAttr {
        ino: inode,
        size: match inode_data {
            InodeData::Regular(_, size, _) => *size as u64,
            InodeData::Symlink(target) => target.len() as u64,
            InodeData::Directory(DirectoryInodeData::Sparse(_, size)) => *size as u64,
            InodeData::Directory(DirectoryInodeData::Populated(_, ref children)) => {
                children.len() as u64
            }
        },
        // FUTUREWORK: play with this numbers, as it affects read sizes for client applications.
        blksize: 1024,
        blocks: 0,
        atime: SystemTime::UNIX_EPOCH,
        mtime: SystemTime::UNIX_EPOCH,
        ctime: SystemTime::UNIX_EPOCH,
        crtime: SystemTime::UNIX_EPOCH,
        kind: inode_data.into(),
        perm: match inode_data {
            InodeData::Regular(_, _, false) => 0o444, // no-executable files
            InodeData::Regular(_, _, true) => 0o555,  // executable files
            InodeData::Symlink(_) => 0o444,
            InodeData::Directory(..) => 0o555,
        },
        nlink: 0,
        uid: 0,
        gid: 0,
        rdev: 0,
        flags: 0,
    }
}
