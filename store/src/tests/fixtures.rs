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
}
