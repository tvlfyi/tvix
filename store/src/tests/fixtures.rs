use crate::proto::{self, Directory, DirectoryNode, FileNode, SymlinkNode};
use lazy_static::lazy_static;

pub const HELLOWORLD_BLOB_CONTENTS: &[u8] = b"Hello World!";
pub const EMPTY_BLOB_CONTENTS: &[u8] = b"";

lazy_static! {
    pub static ref DUMMY_DIGEST: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
    pub static ref DUMMY_DATA_1: Vec<u8> = vec![0x01, 0x02, 0x03];
    pub static ref DUMMY_DATA_2: Vec<u8> = vec![0x04, 0x05];
    pub static ref HELLOWORLD_BLOB_DIGEST: Vec<u8> =
        blake3::hash(HELLOWORLD_BLOB_CONTENTS).as_bytes().to_vec();
    pub static ref EMPTY_BLOB_DIGEST: Vec<u8> =
        blake3::hash(EMPTY_BLOB_CONTENTS).as_bytes().to_vec();

    // 2 bytes
    pub static ref BLOB_A: Vec<u8> = vec![0x00, 0x01];
    pub static ref BLOB_A_DIGEST: Vec<u8> = blake3::hash(&BLOB_A).as_bytes().to_vec();

    // 1MB
    pub static ref BLOB_B: Vec<u8> = (0..255).collect::<Vec<u8>>().repeat(4 * 1024);
    pub static ref BLOB_B_DIGEST: Vec<u8> = blake3::hash(&BLOB_B).as_bytes().to_vec();

    // Directories
    pub static ref DIRECTORY_WITH_KEEP: proto::Directory = proto::Directory {
        directories: vec![],
        files: vec![FileNode {
            name: ".keep".to_string(),
            digest: EMPTY_BLOB_DIGEST.to_vec(),
            size: 0,
            executable: false,
        }],
        symlinks: vec![],
    };
    pub static ref DIRECTORY_COMPLICATED: proto::Directory = proto::Directory {
        directories: vec![DirectoryNode {
            name: "keep".to_string(),
            digest: DIRECTORY_WITH_KEEP.digest(),
            size: DIRECTORY_WITH_KEEP.size(),
        }],
        files: vec![FileNode {
            name: ".keep".to_string(),
            digest: EMPTY_BLOB_DIGEST.to_vec(),
            size: 0,
            executable: false,
        }],
        symlinks: vec![SymlinkNode {
            name: "aa".to_string(),
            target: "/nix/store/somewhereelse".to_string(),
        }],
    };
    pub static ref DIRECTORY_A: Directory = Directory::default();
    pub static ref DIRECTORY_B: Directory = Directory {
        directories: vec![DirectoryNode {
            name: "a".to_string(),
            digest: DIRECTORY_A.digest(),
            size: DIRECTORY_A.size(),
        }],
        ..Default::default()
    };
    pub static ref DIRECTORY_C: Directory = Directory {
        directories: vec![
            DirectoryNode {
                name: "a".to_string(),
                digest: DIRECTORY_A.digest(),
                size: DIRECTORY_A.size(),
            },
            DirectoryNode {
                name: "a'".to_string(),
                digest: DIRECTORY_A.digest(),
                size: DIRECTORY_A.size(),
            }
        ],
        ..Default::default()
    };

    // output hash
    pub static ref DUMMY_OUTPUT_HASH: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00
    ];
}
