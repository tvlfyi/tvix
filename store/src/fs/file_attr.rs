use super::inodes::{DirectoryInodeData, InodeData};
use fuse_backend_rs::abi::fuse_abi::Attr;

/// The [Attr] describing the root
pub const ROOT_FILE_ATTR: Attr = Attr {
    ino: fuse_backend_rs::api::filesystem::ROOT_ID,
    size: 0,
    blksize: 1024,
    blocks: 0,
    mode: libc::S_IFDIR as u32 | 0o555,
    atime: 0,
    mtime: 0,
    ctime: 0,
    atimensec: 0,
    mtimensec: 0,
    ctimensec: 0,
    nlink: 0,
    uid: 0,
    gid: 0,
    rdev: 0,
    flags: 0,
    #[cfg(target_os = "macos")]
    crtime: 0,
    #[cfg(target_os = "macos")]
    crtimensec: 0,
    #[cfg(target_os = "macos")]
    padding: 0,
};

/// for given &Node and inode, construct an [Attr]
pub fn gen_file_attr(inode_data: &InodeData, inode: u64) -> Attr {
    Attr {
        ino: inode,
        // FUTUREWORK: play with this numbers, as it affects read sizes for client applications.
        blocks: 1024,
        size: match inode_data {
            InodeData::Regular(_, size, _) => *size as u64,
            InodeData::Symlink(target) => target.len() as u64,
            InodeData::Directory(DirectoryInodeData::Sparse(_, size)) => *size as u64,
            InodeData::Directory(DirectoryInodeData::Populated(_, ref children)) => {
                children.len() as u64
            }
        },
        mode: match inode_data {
            InodeData::Regular(_, _, false) => libc::S_IFREG as u32 | 0o444, // no-executable files
            InodeData::Regular(_, _, true) => libc::S_IFREG as u32 | 0o555,  // executable files
            InodeData::Symlink(_) => libc::S_IFLNK as u32 | 0o444,
            InodeData::Directory(_) => libc::S_IFDIR as u32 | 0o555,
        },
        ..Default::default()
    }
}
