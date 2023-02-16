use crate::blobservice::BlobService;
use crate::blobservice::SledBlobService;
use crate::chunkservice::ChunkService;
use crate::chunkservice::SledChunkService;
use crate::directoryservice::DirectoryService;
use crate::directoryservice::SledDirectoryService;
use crate::nar::NARRenderer;
use crate::proto;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use lazy_static::lazy_static;
use std::path::Path;
use tempfile::TempDir;

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

fn gen_blob_service(p: &Path) -> impl BlobService {
    SledBlobService::new(p.join("blobs")).unwrap()
}

fn gen_chunk_service(p: &Path) -> impl ChunkService + Clone {
    SledChunkService::new(p.join("chunks")).unwrap()
}

fn gen_directory_service(p: &Path) -> impl DirectoryService {
    SledDirectoryService::new(p.join("directories")).unwrap()
}

#[test]
fn single_symlink() {
    let tmpdir = TempDir::new().unwrap();
    let renderer = NARRenderer::new(
        gen_blob_service(tmpdir.path()),
        gen_chunk_service(tmpdir.path()),
        gen_directory_service(tmpdir.path()),
    );
    // don't put anything in the stores, as we don't actually do any requests.

    let mut buf: Vec<u8> = vec![];

    renderer
        .write_nar(
            &mut buf,
            crate::proto::node::Node::Symlink(SymlinkNode {
                name: "doesntmatter".to_string(),
                target: "/nix/store/somewhereelse".to_string(),
            }),
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
}

#[test]
fn single_file() {
    let tmpdir = TempDir::new().unwrap();

    let blob_service = gen_blob_service(tmpdir.path());
    let chunk_service = gen_chunk_service(tmpdir.path());

    chunk_service
        .put(HELLOWORLD_BLOB_CONTENTS.to_vec())
        .unwrap();

    blob_service
        .put(
            &HELLOWORLD_BLOB_DIGEST,
            proto::BlobMeta {
                chunks: vec![proto::blob_meta::ChunkMeta {
                    digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                    size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
                }],
                ..Default::default()
            },
        )
        .unwrap();

    let renderer = NARRenderer::new(
        blob_service,
        chunk_service,
        gen_directory_service(tmpdir.path()),
    );
    let mut buf: Vec<u8> = vec![];

    renderer
        .write_nar(
            &mut buf,
            crate::proto::node::Node::File(FileNode {
                name: "doesntmatter".to_string(),
                digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
                executable: false,
            }),
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
}

#[test]
fn test_complicated() {
    let tmpdir = TempDir::new().unwrap();

    let blob_service = gen_blob_service(tmpdir.path());
    let chunk_service = gen_chunk_service(tmpdir.path());
    let directory_service = gen_directory_service(tmpdir.path());

    // put all data into the stores.
    for blob_contents in [HELLOWORLD_BLOB_CONTENTS, EMPTY_BLOB_CONTENTS] {
        let digest = chunk_service.put(blob_contents.to_vec()).unwrap();

        blob_service
            .put(
                &digest,
                proto::BlobMeta {
                    chunks: vec![proto::blob_meta::ChunkMeta {
                        digest: digest.to_vec(),
                        size: blob_contents.len() as u32,
                    }],
                    ..Default::default()
                },
            )
            .unwrap();
    }

    directory_service.put(DIRECTORY_WITH_KEEP.clone()).unwrap();
    directory_service
        .put(DIRECTORY_COMPLICATED.clone())
        .unwrap();

    let renderer = NARRenderer::new(blob_service, chunk_service, directory_service);
    let mut buf: Vec<u8> = vec![];

    renderer
        .write_nar(
            &mut buf,
            crate::proto::node::Node::Directory(DirectoryNode {
                name: "doesntmatter".to_string(),
                digest: DIRECTORY_COMPLICATED.digest(),
                size: DIRECTORY_COMPLICATED.size(),
            }),
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
}
