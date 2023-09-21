use crate::{
    proto::{self, Directory, DirectoryNode, FileNode, SymlinkNode},
    B3Digest,
};
use lazy_static::lazy_static;

pub const HELLOWORLD_BLOB_CONTENTS: &[u8] = b"Hello World!";
pub const EMPTY_BLOB_CONTENTS: &[u8] = b"";

lazy_static! {
    pub static ref DUMMY_DIGEST: B3Digest = {
        let u: &[u8; 32] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        u.into()
    };
    pub static ref DUMMY_DIGEST_2: B3Digest = {
        let u: &[u8; 32] = &[
            0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        u.into()
    };
    pub static ref DUMMY_DATA_1: bytes::Bytes = vec![0x01, 0x02, 0x03].into();
    pub static ref DUMMY_DATA_2: bytes::Bytes = vec![0x04, 0x05].into();

    pub static ref HELLOWORLD_BLOB_DIGEST: B3Digest =
        blake3::hash(HELLOWORLD_BLOB_CONTENTS).as_bytes().into();
    pub static ref EMPTY_BLOB_DIGEST: B3Digest =
        blake3::hash(EMPTY_BLOB_CONTENTS).as_bytes().into();

    // 2 bytes
    pub static ref BLOB_A: bytes::Bytes = vec![0x00, 0x01].into();
    pub static ref BLOB_A_DIGEST: B3Digest = blake3::hash(&BLOB_A).as_bytes().into();

    // 1MB
    pub static ref BLOB_B: bytes::Bytes = (0..255).collect::<Vec<u8>>().repeat(4 * 1024).into();
    pub static ref BLOB_B_DIGEST: B3Digest = blake3::hash(&BLOB_B).as_bytes().into();

    // Directories
    pub static ref DIRECTORY_WITH_KEEP: proto::Directory = proto::Directory {
        directories: vec![],
        files: vec![FileNode {
            name: b".keep".to_vec().into(),
            digest: EMPTY_BLOB_DIGEST.clone().into(),
            size: 0,
            executable: false,
        }],
        symlinks: vec![],
    };
    pub static ref DIRECTORY_COMPLICATED: proto::Directory = proto::Directory {
        directories: vec![DirectoryNode {
            name: b"keep".to_vec().into(),
            digest: DIRECTORY_WITH_KEEP.digest().into(),
            size: DIRECTORY_WITH_KEEP.size(),
        }],
        files: vec![FileNode {
            name: b".keep".to_vec().into(),
            digest: EMPTY_BLOB_DIGEST.clone().into(),
            size: 0,
            executable: false,
        }],
        symlinks: vec![SymlinkNode {
            name: b"aa".to_vec().into(),
            target: b"/nix/store/somewhereelse".to_vec().into(),
        }],
    };
    pub static ref DIRECTORY_A: Directory = Directory::default();
    pub static ref DIRECTORY_B: Directory = Directory {
        directories: vec![DirectoryNode {
            name: b"a".to_vec().into(),
            digest: DIRECTORY_A.digest().into(),
            size: DIRECTORY_A.size(),
        }],
        ..Default::default()
    };
    pub static ref DIRECTORY_C: Directory = Directory {
        directories: vec![
            DirectoryNode {
                name: b"a".to_vec().into(),
                digest: DIRECTORY_A.digest().into(),
                size: DIRECTORY_A.size(),
            },
            DirectoryNode {
                name: b"a'".to_vec().into(),
                digest: DIRECTORY_A.digest().into(),
                size: DIRECTORY_A.size(),
            }
        ],
        ..Default::default()
    };
}
