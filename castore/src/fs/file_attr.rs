#![allow(clippy::unnecessary_cast)] // libc::S_IFDIR is u32 on Linux and u16 on MacOS

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
