use lazy_static::lazy_static;
use rstest::*;
use std::sync::Arc;
pub use tvix_castore::fixtures::*;
use tvix_castore::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    proto as castorepb,
};

use crate::proto::{
    nar_info::{ca, Ca},
    NarInfo, PathInfo,
};

pub const DUMMY_NAME: &str = "00000000000000000000000000000000-dummy";
pub const DUMMY_OUTPUT_HASH: [u8; 20] = [0; 20];

lazy_static! {
    /// The NAR representation of a symlink pointing to `/nix/store/somewhereelse`
    pub static ref NAR_CONTENTS_SYMLINK: Vec<u8> = vec![
        13, 0, 0, 0, 0, 0, 0, 0, b'n', b'i', b'x', b'-', b'a', b'r', b'c', b'h', b'i', b'v', b'e', b'-', b'1', 0,
        0, 0, // "nix-archive-1"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b't', b'y', b'p', b'e', 0, 0, 0, 0, // "type"
        7, 0, 0, 0, 0, 0, 0, 0, b's', b'y', b'm', b'l', b'i', b'n', b'k', 0, // "symlink"
        6, 0, 0, 0, 0, 0, 0, 0, b't', b'a', b'r', b'g', b'e', b't', 0, 0, // target
        24, 0, 0, 0, 0, 0, 0, 0, b'/', b'n', b'i', b'x', b'/', b's', b't', b'o', b'r', b'e', b'/', b's', b'o',
        b'm', b'e', b'w', b'h', b'e', b'r', b'e', b'e', b'l', b's',
        b'e', // "/nix/store/somewhereelse"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0 // ")"
    ];

    /// The NAR representation of a regular file with the contents "Hello World!"
    pub static ref NAR_CONTENTS_HELLOWORLD: Vec<u8> = vec![
        13, 0, 0, 0, 0, 0, 0, 0, b'n', b'i', b'x', b'-', b'a', b'r', b'c', b'h', b'i', b'v', b'e', b'-', b'1', 0,
        0, 0, // "nix-archive-1"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b't', b'y', b'p', b'e', 0, 0, 0, 0, // "type"
        7, 0, 0, 0, 0, 0, 0, 0, b'r', b'e', b'g', b'u', b'l', b'a', b'r', 0, // "regular"
        8, 0, 0, 0, 0, 0, 0, 0, b'c', b'o', b'n', b't', b'e', b'n', b't', b's', // "contents"
        12, 0, 0, 0, 0, 0, 0, 0, b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd', b'!', 0, 0,
        0, 0, // "Hello World!"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0 // ")"
    ];

    /// The NAR representation of a more complicated directory structure.
    pub static ref NAR_CONTENTS_COMPLICATED: Vec<u8> = vec![
        13, 0, 0, 0, 0, 0, 0, 0, b'n', b'i', b'x', b'-', b'a', b'r', b'c', b'h', b'i', b'v', b'e', b'-', b'1', 0,
        0, 0, // "nix-archive-1"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b't', b'y', b'p', b'e', 0, 0, 0, 0, // "type"
        9, 0, 0, 0, 0, 0, 0, 0, b'd', b'i', b'r', b'e', b'c', b't', b'o', b'r', b'y', 0, 0, 0, 0, 0, 0, 0, // "directory"
        5, 0, 0, 0, 0, 0, 0, 0, b'e', b'n', b't', b'r', b'y', 0, 0, 0, // "entry"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b'n', b'a', b'm', b'e', 0, 0, 0, 0, // "name"
        5, 0, 0, 0, 0, 0, 0, 0, b'.', b'k', b'e', b'e', b'p', 0, 0, 0, // ".keep"
        4, 0, 0, 0, 0, 0, 0, 0, b'n', b'o', b'd', b'e', 0, 0, 0, 0, // "node"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b't', b'y', b'p', b'e', 0, 0, 0, 0, // "type"
        7, 0, 0, 0, 0, 0, 0, 0, b'r', b'e', b'g', b'u', b'l', b'a', b'r', 0, // "regular"
        8, 0, 0, 0, 0, 0, 0, 0, b'c', b'o', b'n', b't', b'e', b'n', b't', b's', // "contents"
        0, 0, 0, 0, 0, 0, 0, 0, // ""
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        5, 0, 0, 0, 0, 0, 0, 0, b'e', b'n', b't', b'r', b'y', 0, 0, 0, // "entry"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b'n', b'a', b'm', b'e', 0, 0, 0, 0, // "name"
        2, 0, 0, 0, 0, 0, 0, 0, b'a', b'a', 0, 0, 0, 0, 0, 0, // "aa"
        4, 0, 0, 0, 0, 0, 0, 0, b'n', b'o', b'd', b'e', 0, 0, 0, 0, // "node"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b't', b'y', b'p', b'e', 0, 0, 0, 0, // "type"
        7, 0, 0, 0, 0, 0, 0, 0, b's', b'y', b'm', b'l', b'i', b'n', b'k', 0, // "symlink"
        6, 0, 0, 0, 0, 0, 0, 0, b't', b'a', b'r', b'g', b'e', b't', 0, 0, // target
        24, 0, 0, 0, 0, 0, 0, 0, b'/', b'n', b'i', b'x', b'/', b's', b't', b'o', b'r', b'e', b'/', b's', b'o',
        b'm', b'e', b'w', b'h', b'e', b'r', b'e', b'e', b'l', b's',
        b'e', // "/nix/store/somewhereelse"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        5, 0, 0, 0, 0, 0, 0, 0, b'e', b'n', b't', b'r', b'y', 0, 0, 0, // "entry"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b'n', b'a', b'm', b'e', 0, 0, 0, 0, // "name"
        4, 0, 0, 0, 0, 0, 0, 0, b'k', b'e', b'e', b'p', 0, 0, 0, 0, // "keep"
        4, 0, 0, 0, 0, 0, 0, 0, b'n', b'o', b'd', b'e', 0, 0, 0, 0, // "node"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b't', b'y', b'p', b'e', 0, 0, 0, 0, // "type"
        9, 0, 0, 0, 0, 0, 0, 0, b'd', b'i', b'r', b'e', b'c', b't', b'o', b'r', b'y', 0, 0, 0, 0, 0, 0, 0, // "directory"
        5, 0, 0, 0, 0, 0, 0, 0, b'e', b'n', b't', b'r', b'y', 0, 0, 0, // "entry"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b'n', b'a', b'm', b'e', 0, 0, 0, 0, // "name"
        5, 0, 0, 0, 0, 0, 0, 0, 46, 107, 101, 101, 112, 0, 0, 0, // ".keep"
        4, 0, 0, 0, 0, 0, 0, 0, 110, 111, 100, 101, 0, 0, 0, 0, // "node"
        1, 0, 0, 0, 0, 0, 0, 0, b'(', 0, 0, 0, 0, 0, 0, 0, // "("
        4, 0, 0, 0, 0, 0, 0, 0, b't', b'y', b'p', b'e', 0, 0, 0, 0, // "type"
        7, 0, 0, 0, 0, 0, 0, 0, b'r', b'e', b'g', b'u', b'l', b'a', b'r', 0, // "regular"
        8, 0, 0, 0, 0, 0, 0, 0, b'c', b'o', b'n', b't', b'e', b'n', b't', b's', // "contents"
        0, 0, 0, 0, 0, 0, 0, 0, // ""
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
        1, 0, 0, 0, 0, 0, 0, 0, b')', 0, 0, 0, 0, 0, 0, 0, // ")"
    ];

    /// A PathInfo message without .narinfo populated.
    pub static ref PATH_INFO_WITHOUT_NARINFO : PathInfo = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Directory(castorepb::DirectoryNode {
                name: DUMMY_NAME.into(),
                digest: DUMMY_DIGEST.clone().into(),
                size: 0,
            })),
        }),
        references: vec![DUMMY_OUTPUT_HASH.as_slice().into()],
        narinfo: None,
    };

    /// A PathInfo message with .narinfo populated.
    /// The references in `narinfo.reference_names` aligns with what's in
    /// `references`.
    pub static ref PATH_INFO_WITH_NARINFO : PathInfo = PathInfo {
        narinfo: Some(NarInfo {
            nar_size: 0,
            nar_sha256: DUMMY_DIGEST.clone().into(),
            signatures: vec![],
            reference_names: vec![DUMMY_NAME.to_string()],
            deriver: None,
            ca: Some(Ca { r#type: ca::Hash::NarSha256.into(), digest:  DUMMY_DIGEST.clone().into() })
        }),
      ..PATH_INFO_WITHOUT_NARINFO.clone()
    };
}

#[fixture]
pub(crate) fn blob_service() -> Arc<dyn BlobService> {
    Arc::from(MemoryBlobService::default())
}

#[fixture]
pub(crate) fn directory_service() -> Arc<dyn DirectoryService> {
    Arc::from(MemoryDirectoryService::default())
}
