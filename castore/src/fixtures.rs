use bytes::Bytes;
use std::sync::LazyLock;

use crate::{B3Digest, Directory, Node};

pub const HELLOWORLD_BLOB_CONTENTS: &[u8] = b"Hello World!";
pub const EMPTY_BLOB_CONTENTS: &[u8] = b"";

pub static DUMMY_DIGEST: LazyLock<B3Digest> = LazyLock::new(|| (&[0u8; 32]).into());
pub static DUMMY_DIGEST_2: LazyLock<B3Digest> = LazyLock::new(|| {
    let mut u = [0u8; 32];
    u[0] = 0x10;
    (&u).into()
});
pub static DUMMY_DATA_1: LazyLock<Bytes> = LazyLock::new(|| vec![0x01, 0x02, 0x03].into());
pub static DUMMY_DATA_2: LazyLock<Bytes> = LazyLock::new(|| vec![0x04, 0x05].into());

pub static HELLOWORLD_BLOB_DIGEST: LazyLock<B3Digest> =
    LazyLock::new(|| blake3::hash(HELLOWORLD_BLOB_CONTENTS).as_bytes().into());
pub static EMPTY_BLOB_DIGEST: LazyLock<B3Digest> =
    LazyLock::new(|| blake3::hash(EMPTY_BLOB_CONTENTS).as_bytes().into());

// 2 bytes
pub static BLOB_A: LazyLock<Bytes> = LazyLock::new(|| vec![0x00, 0x01].into());
pub static BLOB_A_DIGEST: LazyLock<B3Digest> =
    LazyLock::new(|| blake3::hash(&BLOB_A).as_bytes().into());

// 1MB
pub static BLOB_B: LazyLock<Bytes> =
    LazyLock::new(|| (0..255).collect::<Vec<u8>>().repeat(4 * 1024).into());
pub static BLOB_B_DIGEST: LazyLock<B3Digest> =
    LazyLock::new(|| blake3::hash(&BLOB_B).as_bytes().into());

// Directories
pub static DIRECTORY_WITH_KEEP: LazyLock<Directory> = LazyLock::new(|| {
    Directory::try_from_iter([(
        ".keep".try_into().unwrap(),
        Node::File {
            digest: EMPTY_BLOB_DIGEST.clone(),
            size: 0,
            executable: false,
        },
    )])
    .unwrap()
});
pub static DIRECTORY_COMPLICATED: LazyLock<Directory> = LazyLock::new(|| {
    Directory::try_from_iter([
        (
            "keep".try_into().unwrap(),
            Node::Directory {
                digest: DIRECTORY_WITH_KEEP.digest(),
                size: DIRECTORY_WITH_KEEP.size(),
            },
        ),
        (
            ".keep".try_into().unwrap(),
            Node::File {
                digest: EMPTY_BLOB_DIGEST.clone(),
                size: 0,
                executable: false,
            },
        ),
        (
            "aa".try_into().unwrap(),
            Node::Symlink {
                target: "/nix/store/somewhereelse".try_into().unwrap(),
            },
        ),
    ])
    .unwrap()
});
pub static DIRECTORY_A: LazyLock<Directory> = LazyLock::new(Directory::new);
pub static DIRECTORY_B: LazyLock<Directory> = LazyLock::new(|| {
    Directory::try_from_iter([(
        "a".try_into().unwrap(),
        Node::Directory {
            digest: DIRECTORY_A.digest(),
            size: DIRECTORY_A.size(),
        },
    )])
    .unwrap()
});
pub static DIRECTORY_C: LazyLock<Directory> = LazyLock::new(|| {
    Directory::try_from_iter([
        (
            "a".try_into().unwrap(),
            Node::Directory {
                digest: DIRECTORY_A.digest(),
                size: DIRECTORY_A.size(),
            },
        ),
        (
            "a'".try_into().unwrap(),
            Node::Directory {
                digest: DIRECTORY_A.digest(),
                size: DIRECTORY_A.size(),
            },
        ),
    ])
    .unwrap()
});
pub static DIRECTORY_D: LazyLock<Directory> = LazyLock::new(|| {
    Directory::try_from_iter([
        (
            "a".try_into().unwrap(),
            Node::Directory {
                digest: DIRECTORY_A.digest(),
                size: DIRECTORY_A.size(),
            },
        ),
        (
            "b".try_into().unwrap(),
            Node::Directory {
                digest: DIRECTORY_B.digest(),
                size: DIRECTORY_B.size(),
            },
        ),
    ])
    .unwrap()
});
