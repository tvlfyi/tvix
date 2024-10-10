use crate::pathinfoservice::PathInfo;
use lazy_static::lazy_static;
use nix_compat::nixhash::{CAHash, NixHash};
use nix_compat::store_path::StorePath;
use rstest::{self, *};
use rstest_reuse::*;
use std::io;
use std::sync::Arc;
use tvix_castore::fixtures::{
    DIRECTORY_COMPLICATED, DIRECTORY_WITH_KEEP, DUMMY_DIGEST, EMPTY_BLOB_CONTENTS,
    EMPTY_BLOB_DIGEST, HELLOWORLD_BLOB_CONTENTS, HELLOWORLD_BLOB_DIGEST,
};
use tvix_castore::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    Node,
};

pub const DUMMY_PATH_STR: &str = "00000000000000000000000000000000-dummy";
pub const DUMMY_PATH_DIGEST: [u8; 20] = [0; 20];

lazy_static! {
    pub static ref DUMMY_PATH: StorePath<String> = StorePath::from_name_and_digest_fixed("dummy", DUMMY_PATH_DIGEST).unwrap();

    pub static ref CASTORE_NODE_SYMLINK: Node = Node::Symlink {
        target: "/nix/store/somewhereelse".try_into().unwrap(),
    };

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

    pub static ref CASTORE_NODE_HELLOWORLD: Node = Node::File {
        digest: HELLOWORLD_BLOB_DIGEST.clone(),
        size: HELLOWORLD_BLOB_CONTENTS.len() as u64,
        executable: false,
    };

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

    pub static ref CASTORE_NODE_TOO_BIG: Node = Node::File {
        digest: HELLOWORLD_BLOB_DIGEST.clone(),
        size: 42, // <- note the wrong size here!
        executable: false,
    };
    pub static ref CASTORE_NODE_TOO_SMALL: Node = Node::File {
        digest: HELLOWORLD_BLOB_DIGEST.clone(),
        size: 2, // <- note the wrong size here!
        executable: false,
    };

    pub static ref CASTORE_NODE_COMPLICATED: Node = Node::Directory {
        digest: DIRECTORY_COMPLICATED.digest(),
        size: DIRECTORY_COMPLICATED.size(),
    };

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

    /// A PathInfo message
    pub static ref PATH_INFO: PathInfo = PathInfo {
        store_path: DUMMY_PATH.clone(),
        node: tvix_castore::Node::Directory {
            digest: DUMMY_DIGEST.clone(),
            size: 0,
        },
        references: vec![DUMMY_PATH.clone()],
        nar_sha256: [0; 32],
        nar_size: 0,
        signatures: vec![],
        deriver: None,
        ca: Some(CAHash::Nar(NixHash::Sha256([0; 32]))),
    };
}

#[fixture]
pub(crate) fn blob_service() -> Arc<dyn BlobService> {
    Arc::from(MemoryBlobService::default())
}

#[fixture]
pub(crate) async fn blob_service_with_contents() -> Arc<dyn BlobService> {
    let blob_service = Arc::from(MemoryBlobService::default());
    for (blob_contents, blob_digest) in [
        (EMPTY_BLOB_CONTENTS, &*EMPTY_BLOB_DIGEST),
        (HELLOWORLD_BLOB_CONTENTS, &*HELLOWORLD_BLOB_DIGEST),
    ] {
        // put all data into the stores.
        // insert blob into the store
        let mut writer = blob_service.open_write().await;
        tokio::io::copy(&mut io::Cursor::new(blob_contents), &mut writer)
            .await
            .unwrap();
        assert_eq!(blob_digest.clone(), writer.close().await.unwrap());
    }
    blob_service
}

#[fixture]
pub(crate) fn directory_service() -> Arc<dyn DirectoryService> {
    Arc::from(MemoryDirectoryService::default())
}

#[fixture]
pub(crate) async fn directory_service_with_contents() -> Arc<dyn DirectoryService> {
    let directory_service = Arc::from(MemoryDirectoryService::default());
    for directory in [&*DIRECTORY_WITH_KEEP, &*DIRECTORY_COMPLICATED] {
        directory_service.put(directory.clone()).await.unwrap();
    }
    directory_service
}

#[template]
#[rstest]
#[case::symlink    (&*CASTORE_NODE_SYMLINK,     Ok(Ok(&*NAR_CONTENTS_SYMLINK)))]
#[case::helloworld (&*CASTORE_NODE_HELLOWORLD,  Ok(Ok(&*NAR_CONTENTS_HELLOWORLD)))]
#[case::too_big    (&*CASTORE_NODE_TOO_BIG,     Ok(Err(io::ErrorKind::UnexpectedEof)))]
#[case::too_small  (&*CASTORE_NODE_TOO_SMALL,   Ok(Err(io::ErrorKind::InvalidInput)))]
#[case::complicated(&*CASTORE_NODE_COMPLICATED, Ok(Ok(&*NAR_CONTENTS_COMPLICATED)))]
fn castore_fixtures_template(
    #[case] test_input: &Node,
    #[case] test_output: Result<Result<&Vec<u8>, io::ErrorKind>, crate::nar::RenderError>,
) {
}
