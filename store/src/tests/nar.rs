use data_encoding::BASE64;

use crate::client::StoreClient;
use crate::nar::write_nar;
use crate::proto;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use lazy_static::lazy_static;

const HELLOWORLD_BLOB_CONTENTS: &[u8] = b"Hello World!";
const EMPTY_BLOB_CONTENTS: &[u8] = b"";

lazy_static! {
    static ref HELLOWORLD_BLOB_DIGEST: Vec<u8> =
        blake3::hash(HELLOWORLD_BLOB_CONTENTS).as_bytes().to_vec();
    static ref EMPTY_BLOB_DIGEST: Vec<u8> = blake3::hash(EMPTY_BLOB_CONTENTS).as_bytes().to_vec();
    static ref DIRECTORY_WITH_KEEP: proto::Directory = proto::Directory {
        directories: vec![],
        files: vec![FileNode {
            name: ".keep".to_string(),
            digest: EMPTY_BLOB_DIGEST.to_vec(),
            size: 0,
            executable: false,
        }],
        symlinks: vec![],
    };
    static ref DIRECTORY_COMPLICATED: proto::Directory = proto::Directory {
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
}

/// A Store client that fails if you ask it for a blob or a directory
#[derive(Default)]
struct FailingStoreClient {}

impl StoreClient for FailingStoreClient {
    fn open_blob(&self, digest: Vec<u8>) -> std::io::Result<Box<dyn std::io::BufRead>> {
        panic!(
            "open_blob should never be called, but was called with {}",
            BASE64.encode(&digest),
        );
    }

    fn get_directory(&self, digest: Vec<u8>) -> std::io::Result<Option<proto::Directory>> {
        panic!(
            "get_directory should never be called, but was called with {}",
            BASE64.encode(&digest),
        );
    }
}

/// Only allow a request for a blob with [HELLOWORLD_BLOB_DIGEST]
/// panic on everything else.
#[derive(Default)]
struct HelloWorldBlobStoreClient {}

impl StoreClient for HelloWorldBlobStoreClient {
    fn open_blob(&self, digest: Vec<u8>) -> std::io::Result<Box<dyn std::io::BufRead>> {
        if digest != HELLOWORLD_BLOB_DIGEST.to_vec() {
            panic!("open_blob called with {}", BASE64.encode(&digest));
        }

        let b: Box<&[u8]> = Box::new(&HELLOWORLD_BLOB_CONTENTS);

        Ok(b)
    }

    fn get_directory(&self, digest: Vec<u8>) -> std::io::Result<Option<proto::Directory>> {
        panic!(
            "get_directory should never be called, but was called with {}",
            BASE64.encode(&digest),
        );
    }
}

/// Allow blob requests for [HELLOWORLD_BLOB_DIGEST] and EMPTY_BLOB_DIGEST, and
/// allow DIRECTORY_WITH_KEEP and DIRECTORY_COMPLICATED.
#[derive(Default)]
struct SomeDirectoryStoreClient {}

impl StoreClient for SomeDirectoryStoreClient {
    fn open_blob(&self, digest: Vec<u8>) -> std::io::Result<Box<dyn std::io::BufRead>> {
        if digest == HELLOWORLD_BLOB_DIGEST.to_vec() {
            let b: Box<&[u8]> = Box::new(&HELLOWORLD_BLOB_CONTENTS);
            return Ok(b);
        }
        if digest == EMPTY_BLOB_DIGEST.to_vec() {
            let b: Box<&[u8]> = Box::new(&EMPTY_BLOB_CONTENTS);
            return Ok(b);
        }
        panic!("open_blob called with {}", BASE64.encode(&digest));
    }

    fn get_directory(&self, digest: Vec<u8>) -> std::io::Result<Option<proto::Directory>> {
        if digest == DIRECTORY_WITH_KEEP.digest() {
            return Ok(Some(DIRECTORY_WITH_KEEP.clone()));
        }
        if digest == DIRECTORY_COMPLICATED.digest() {
            return Ok(Some(DIRECTORY_COMPLICATED.clone()));
        }
        panic!("get_directory called with {}", BASE64.encode(&digest));
    }
}

#[tokio::test]
async fn single_symlink() -> anyhow::Result<()> {
    let mut buf: Vec<u8> = vec![];
    let mut store_client = FailingStoreClient::default();

    write_nar(
        &mut buf,
        crate::proto::node::Node::Symlink(SymlinkNode {
            name: "doesntmatter".to_string(),
            target: "/nix/store/somewhereelse".to_string(),
        }),
        &mut store_client,
    )
    .expect("must succeed");

    assert_eq!(
        buf,
        vec![
            13, 0, 0, 0, 0, 0, 0, 0, 110, 105, 120, 45, 97, 114, 99, 104, 105, 118, 101, 45, 49, 0,
            0, 0, // "nix-archive-1"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 116, 121, 112, 101, 0, 0, 0, 0, // "type"
            7, 0, 0, 0, 0, 0, 0, 0, 115, 121, 109, 108, 105, 110, 107, 0, // "symlink"
            6, 0, 0, 0, 0, 0, 0, 0, 116, 97, 114, 103, 101, 116, 0, 0, // target
            24, 0, 0, 0, 0, 0, 0, 0, 47, 110, 105, 120, 47, 115, 116, 111, 114, 101, 47, 115, 111,
            109, 101, 119, 104, 101, 114, 101, 101, 108, 115,
            101, // "/nix/store/somewhereelse"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0 // ")"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn single_file() -> anyhow::Result<()> {
    let mut buf: Vec<u8> = vec![];
    let mut store_client = HelloWorldBlobStoreClient::default();

    write_nar(
        &mut buf,
        crate::proto::node::Node::File(FileNode {
            name: "doesntmatter".to_string(),
            digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
            size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
            executable: false,
        }),
        &mut store_client,
    )
    .expect("must succeed");

    assert_eq!(
        buf,
        vec![
            13, 0, 0, 0, 0, 0, 0, 0, 110, 105, 120, 45, 97, 114, 99, 104, 105, 118, 101, 45, 49, 0,
            0, 0, // "nix-archive-1"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 116, 121, 112, 101, 0, 0, 0, 0, // "type"
            7, 0, 0, 0, 0, 0, 0, 0, 114, 101, 103, 117, 108, 97, 114, 0, // "regular"
            8, 0, 0, 0, 0, 0, 0, 0, 99, 111, 110, 116, 101, 110, 116, 115, // "contents"
            12, 0, 0, 0, 0, 0, 0, 0, 72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100, 33, 0, 0,
            0, 0, // "Hello World!"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0 // ")"
        ]
    );

    Ok(())
}

#[tokio::test]
async fn test_complicated() -> anyhow::Result<()> {
    let mut buf: Vec<u8> = vec![];
    let mut store_client = SomeDirectoryStoreClient::default();

    write_nar(
        &mut buf,
        crate::proto::node::Node::Directory(DirectoryNode {
            name: "doesntmatter".to_string(),
            digest: DIRECTORY_COMPLICATED.digest(),
            size: DIRECTORY_COMPLICATED.size() as u32,
        }),
        &mut store_client,
    )
    .expect("must succeed");

    assert_eq!(
        buf,
        vec![
            13, 0, 0, 0, 0, 0, 0, 0, 110, 105, 120, 45, 97, 114, 99, 104, 105, 118, 101, 45, 49, 0,
            0, 0, // "nix-archive-1"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 116, 121, 112, 101, 0, 0, 0, 0, // "type"
            9, 0, 0, 0, 0, 0, 0, 0, 100, 105, 114, 101, 99, 116, 111, 114, 121, 0, 0, 0, 0, 0, 0,
            0, // "directory"
            5, 0, 0, 0, 0, 0, 0, 0, 101, 110, 116, 114, 121, 0, 0, 0, // "entry"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 110, 97, 109, 101, 0, 0, 0, 0, // "name"
            5, 0, 0, 0, 0, 0, 0, 0, 46, 107, 101, 101, 112, 0, 0, 0, // ".keep"
            4, 0, 0, 0, 0, 0, 0, 0, 110, 111, 100, 101, 0, 0, 0, 0, // "node"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 116, 121, 112, 101, 0, 0, 0, 0, // "type"
            7, 0, 0, 0, 0, 0, 0, 0, 114, 101, 103, 117, 108, 97, 114, 0, // "regular"
            8, 0, 0, 0, 0, 0, 0, 0, 99, 111, 110, 116, 101, 110, 116, 115, 0, 0, 0, 0, 0, 0, 0,
            0, // "contents"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            5, 0, 0, 0, 0, 0, 0, 0, 101, 110, 116, 114, 121, 0, 0, 0, // "entry"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 110, 97, 109, 101, 0, 0, 0, 0, // "name"
            2, 0, 0, 0, 0, 0, 0, 0, 97, 97, 0, 0, 0, 0, 0, 0, // "aa"
            4, 0, 0, 0, 0, 0, 0, 0, 110, 111, 100, 101, 0, 0, 0, 0, // "node"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 116, 121, 112, 101, 0, 0, 0, 0, // "type"
            7, 0, 0, 0, 0, 0, 0, 0, 115, 121, 109, 108, 105, 110, 107, 0, // "symlink"
            6, 0, 0, 0, 0, 0, 0, 0, 116, 97, 114, 103, 101, 116, 0, 0, // "target"
            24, 0, 0, 0, 0, 0, 0, 0, 47, 110, 105, 120, 47, 115, 116, 111, 114, 101, 47, 115, 111,
            109, 101, 119, 104, 101, 114, 101, 101, 108, 115,
            101, //  "/nix/store/somewhereelse"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            5, 0, 0, 0, 0, 0, 0, 0, 101, 110, 116, 114, 121, 0, 0, 0, // "entry"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 110, 97, 109, 101, 0, 0, 0, 0, // "name"
            4, 0, 0, 0, 0, 0, 0, 0, 107, 101, 101, 112, 0, 0, 0, 0, // "keep"
            4, 0, 0, 0, 0, 0, 0, 0, 110, 111, 100, 101, 0, 0, 0, 0, // "node"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 116, 121, 112, 101, 0, 0, 0, 0, // "type"
            9, 0, 0, 0, 0, 0, 0, 0, 100, 105, 114, 101, 99, 116, 111, 114, 121, 0, 0, 0, 0, 0, 0,
            0, // "directory"
            5, 0, 0, 0, 0, 0, 0, 0, 101, 110, 116, 114, 121, 0, 0, 0, // "entry"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 110, 97, 109, 101, 0, 0, 0, 0, // "name"
            5, 0, 0, 0, 0, 0, 0, 0, 46, 107, 101, 101, 112, 0, 0, 0, // ".keep"
            4, 0, 0, 0, 0, 0, 0, 0, 110, 111, 100, 101, 0, 0, 0, 0, // "node"
            1, 0, 0, 0, 0, 0, 0, 0, 40, 0, 0, 0, 0, 0, 0, 0, // "("
            4, 0, 0, 0, 0, 0, 0, 0, 116, 121, 112, 101, 0, 0, 0, 0, // "type"
            7, 0, 0, 0, 0, 0, 0, 0, 114, 101, 103, 117, 108, 97, 114, 0, // "regular"
            8, 0, 0, 0, 0, 0, 0, 0, 99, 111, 110, 116, 101, 110, 116, 115, 0, 0, 0, 0, 0, 0, 0,
            0, // "contents"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0, // ")"
            1, 0, 0, 0, 0, 0, 0, 0, 41, 0, 0, 0, 0, 0, 0, 0 // ")"
        ]
    );

    Ok(())
}
