use crate::blobservice::BlobService;
use crate::blobservice::BlobWriter;
use crate::directoryservice::DirectoryService;
use crate::nar::NARRenderer;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::tests::fixtures::*;
use crate::tests::utils::*;
use std::io;

#[test]
fn single_symlink() {
    let renderer = NARRenderer::new(gen_blob_service(), gen_directory_service());
    // don't put anything in the stores, as we don't actually do any requests.

    let mut buf: Vec<u8> = vec![];

    renderer
        .write_nar(
            &mut buf,
            &crate::proto::node::Node::Symlink(SymlinkNode {
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
    let renderer = NARRenderer::new(gen_blob_service(), gen_directory_service());
    let mut buf: Vec<u8> = vec![];

    let e = renderer
        .write_nar(
            &mut buf,
            &crate::proto::node::Node::File(FileNode {
                name: "doesntmatter".to_string(),
                digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
                executable: false,
            }),
        )
        .expect_err("must fail");

    match e {
        crate::nar::RenderError::NARWriterError(e) => {
            assert_eq!(io::ErrorKind::NotFound, e.kind());
        }
        _ => panic!("unexpected error: {:?}", e),
    }
}

/// Make sure the NAR Renderer fails if the returned blob meta has another size
/// than specified in the proto node.
#[test]
fn single_file_wrong_blob_size() {
    let blob_service = gen_blob_service();

    // insert blob into the store
    let mut writer = blob_service.open_write().unwrap();
    io::copy(
        &mut io::Cursor::new(HELLOWORLD_BLOB_CONTENTS.to_vec()),
        &mut writer,
    )
    .unwrap();
    assert_eq!(HELLOWORLD_BLOB_DIGEST.to_vec(), writer.close().unwrap());

    let renderer = NARRenderer::new(blob_service, gen_directory_service());

    // Test with a root FileNode of a too big size
    {
        let mut buf: Vec<u8> = vec![];
        let e = renderer
            .write_nar(
                &mut buf,
                &crate::proto::node::Node::File(FileNode {
                    name: "doesntmatter".to_string(),
                    digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                    size: 42, // <- note the wrong size here!
                    executable: false,
                }),
            )
            .expect_err("must fail");

        match e {
            crate::nar::RenderError::NARWriterError(e) => {
                assert_eq!(io::ErrorKind::UnexpectedEof, e.kind());
            }
            _ => panic!("unexpected error: {:?}", e),
        }
    }

    // Test with a root FileNode of a too small size
    {
        let mut buf: Vec<u8> = vec![];
        let e = renderer
            .write_nar(
                &mut buf,
                &crate::proto::node::Node::File(FileNode {
                    name: "doesntmatter".to_string(),
                    digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                    size: 2, // <- note the wrong size here!
                    executable: false,
                }),
            )
            .expect_err("must fail");

        match e {
            crate::nar::RenderError::NARWriterError(e) => {
                assert_eq!(io::ErrorKind::InvalidInput, e.kind());
            }
            _ => panic!("unexpected error: {:?}", e),
        }
    }
}

#[test]
fn single_file() {
    let blob_service = gen_blob_service();

    // insert blob into the store
    let mut writer = blob_service.open_write().unwrap();
    io::copy(
        &mut io::Cursor::new(HELLOWORLD_BLOB_CONTENTS.to_vec()),
        &mut writer,
    )
    .unwrap();
    assert_eq!(HELLOWORLD_BLOB_DIGEST.to_vec(), writer.close().unwrap());

    let renderer = NARRenderer::new(blob_service, gen_directory_service());
    let mut buf: Vec<u8> = vec![];

    renderer
        .write_nar(
            &mut buf,
            &crate::proto::node::Node::File(FileNode {
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
    let blob_service = gen_blob_service();
    let directory_service = gen_directory_service();

    // put all data into the stores.
    // insert blob into the store
    let mut writer = blob_service.open_write().unwrap();
    io::copy(
        &mut io::Cursor::new(EMPTY_BLOB_CONTENTS.to_vec()),
        &mut writer,
    )
    .unwrap();
    assert_eq!(EMPTY_BLOB_DIGEST.to_vec(), writer.close().unwrap());

    directory_service.put(DIRECTORY_WITH_KEEP.clone()).unwrap();
    directory_service
        .put(DIRECTORY_COMPLICATED.clone())
        .unwrap();

    let renderer = NARRenderer::new(blob_service, directory_service);
    let mut buf: Vec<u8> = vec![];

    renderer
        .write_nar(
            &mut buf,
            &crate::proto::node::Node::Directory(DirectoryNode {
                name: "doesntmatter".to_string(),
                digest: DIRECTORY_COMPLICATED.digest().to_vec(),
                size: DIRECTORY_COMPLICATED.size(),
            }),
        )
        .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_COMPLICATED.to_vec());
}
