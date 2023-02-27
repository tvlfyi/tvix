use crate::blobservice::BlobService;
use crate::chunkservice::ChunkService;
use crate::directoryservice::DirectoryService;
use crate::nar::NARRenderer;
use crate::proto;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::tests::fixtures::*;
use crate::tests::utils::*;
use tempfile::TempDir;

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

    assert_eq!(buf, NAR_CONTENTS_SYMLINK.to_vec());
}

/// Make sure the NARRenderer fails if the blob size in the proto node doesn't
/// match what's in the store.
#[test]
fn single_file_missing_blob() {
    let tmpdir = TempDir::new().unwrap();

    let blob_service = gen_blob_service(tmpdir.path());
    let chunk_service = gen_chunk_service(tmpdir.path());

    let renderer = NARRenderer::new(
        blob_service,
        chunk_service,
        gen_directory_service(tmpdir.path()),
    );
    let mut buf: Vec<u8> = vec![];

    let e = renderer
        .write_nar(
            &mut buf,
            crate::proto::node::Node::File(FileNode {
                name: "doesntmatter".to_string(),
                digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
                executable: false,
            }),
        )
        .expect_err("must fail");

    if let crate::nar::RenderError::BlobNotFound(actual_digest, _) = e {
        assert_eq!(HELLOWORLD_BLOB_DIGEST.to_vec(), actual_digest);
    } else {
        panic!("unexpected error")
    }
}

/// Make sure the NAR Renderer fails if the returned blob meta has another size
/// than specified in the proto node.
#[test]
fn single_file_wrong_blob_size() {
    let tmpdir = TempDir::new().unwrap();

    let blob_service = gen_blob_service(tmpdir.path());
    let chunk_service = gen_chunk_service(tmpdir.path());

    // insert blob and chunk into the stores
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

    let e = renderer
        .write_nar(
            &mut buf,
            crate::proto::node::Node::File(FileNode {
                name: "doesntmatter".to_string(),
                digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                size: 42, // <- note the wrong size here!
                executable: false,
            }),
        )
        .expect_err("must fail");

    if let crate::nar::RenderError::UnexpectedBlobMeta(digest, _, expected_size, actual_size) = e {
        assert_eq!(
            digest,
            HELLOWORLD_BLOB_DIGEST.to_vec(),
            "expect digest to match"
        );
        assert_eq!(
            expected_size, 42,
            "expected expected size to be what's passed in the request"
        );
        assert_eq!(
            actual_size,
            HELLOWORLD_BLOB_CONTENTS.len() as u32,
            "expected actual size to be correct"
        );
    } else {
        panic!("unexpected error")
    }
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

    assert_eq!(buf, NAR_CONTENTS_HELLOWORLD.to_vec());
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

    assert_eq!(buf, NAR_CONTENTS_COMPLICATED.to_vec());
}
