use crate::nar::calculate_size_and_sha256;
use crate::nar::write_nar;
use crate::tests::fixtures::blob_service;
use crate::tests::fixtures::directory_service;
use crate::tests::fixtures::*;
use rstest::*;
use sha2::{Digest, Sha256};
use std::io;
use std::sync::Arc;
use tokio::io::sink;
use tvix_castore::blobservice::BlobService;
use tvix_castore::directoryservice::DirectoryService;
use tvix_castore::{DirectoryNode, FileNode, Node, SymlinkNode};

#[rstest]
#[tokio::test]
async fn single_symlink(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) {
    let mut buf: Vec<u8> = vec![];

    write_nar(
        &mut buf,
        &Node::Symlink(
            SymlinkNode::new("doesntmatter".into(), "/nix/store/somewhereelse".into()).unwrap(),
        ),
        // don't put anything in the stores, as we don't actually do any requests.
        blob_service,
        directory_service,
    )
    .await
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_SYMLINK.to_vec());
}

/// Make sure the NARRenderer fails if a referred blob doesn't exist.
#[rstest]
#[tokio::test]
async fn single_file_missing_blob(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) {
    let e = write_nar(
        sink(),
        &Node::File(
            FileNode::new(
                "doesntmatter".into(),
                HELLOWORLD_BLOB_DIGEST.clone(),
                HELLOWORLD_BLOB_CONTENTS.len() as u64,
                false,
            )
            .unwrap(),
        ),
        // the blobservice is empty intentionally, to provoke the error.
        blob_service,
        directory_service,
    )
    .await
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
#[rstest]
#[tokio::test]
async fn single_file_wrong_blob_size(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) {
    // insert blob into the store
    let mut writer = blob_service.open_write().await;
    tokio::io::copy(
        &mut io::Cursor::new(HELLOWORLD_BLOB_CONTENTS.to_vec()),
        &mut writer,
    )
    .await
    .unwrap();
    assert_eq!(
        HELLOWORLD_BLOB_DIGEST.clone(),
        writer.close().await.unwrap()
    );

    // Test with a root FileNode of a too big size
    let e = write_nar(
        sink(),
        &Node::File(
            FileNode::new(
                "doesntmatter".into(),
                HELLOWORLD_BLOB_DIGEST.clone(),
                42, // <- note the wrong size here!
                false,
            )
            .unwrap(),
        ),
        blob_service.clone(),
        directory_service.clone(),
    )
    .await
    .expect_err("must fail");

    match e {
        crate::nar::RenderError::NARWriterError(e) => {
            assert_eq!(io::ErrorKind::UnexpectedEof, e.kind());
        }
        _ => panic!("unexpected error: {:?}", e),
    }

    // Test with a root FileNode of a too small size
    let e = write_nar(
        sink(),
        &Node::File(
            FileNode::new(
                "doesntmatter".into(),
                HELLOWORLD_BLOB_DIGEST.clone(),
                2, // <- note the wrong size here!
                false,
            )
            .unwrap(),
        ),
        blob_service,
        directory_service,
    )
    .await
    .expect_err("must fail");

    match e {
        crate::nar::RenderError::NARWriterError(e) => {
            assert_eq!(io::ErrorKind::InvalidInput, e.kind());
        }
        _ => panic!("unexpected error: {:?}", e),
    }
}

#[rstest]
#[tokio::test]
async fn single_file(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) {
    // insert blob into the store
    let mut writer = blob_service.open_write().await;
    tokio::io::copy(&mut io::Cursor::new(HELLOWORLD_BLOB_CONTENTS), &mut writer)
        .await
        .unwrap();

    assert_eq!(
        HELLOWORLD_BLOB_DIGEST.clone(),
        writer.close().await.unwrap()
    );

    let mut buf: Vec<u8> = vec![];

    write_nar(
        &mut buf,
        &Node::File(
            FileNode::new(
                "doesntmatter".into(),
                HELLOWORLD_BLOB_DIGEST.clone(),
                HELLOWORLD_BLOB_CONTENTS.len() as u64,
                false,
            )
            .unwrap(),
        ),
        blob_service,
        directory_service,
    )
    .await
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_HELLOWORLD.to_vec());
}

#[rstest]
#[tokio::test]
async fn test_complicated(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) {
    // put all data into the stores.
    // insert blob into the store
    let mut writer = blob_service.open_write().await;
    tokio::io::copy(&mut io::Cursor::new(EMPTY_BLOB_CONTENTS), &mut writer)
        .await
        .unwrap();
    assert_eq!(EMPTY_BLOB_DIGEST.clone(), writer.close().await.unwrap());

    // insert directories
    directory_service
        .put(DIRECTORY_WITH_KEEP.clone())
        .await
        .unwrap();
    directory_service
        .put(DIRECTORY_COMPLICATED.clone())
        .await
        .unwrap();

    let mut buf: Vec<u8> = vec![];

    write_nar(
        &mut buf,
        &Node::Directory(
            DirectoryNode::new(
                "doesntmatter".into(),
                DIRECTORY_COMPLICATED.digest(),
                DIRECTORY_COMPLICATED.size(),
            )
            .unwrap(),
        ),
        blob_service.clone(),
        directory_service.clone(),
    )
    .await
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_COMPLICATED.to_vec());

    // ensure calculate_nar does return the correct sha256 digest and sum.
    let (nar_size, nar_digest) = calculate_size_and_sha256(
        &Node::Directory(
            DirectoryNode::new(
                "doesntmatter".into(),
                DIRECTORY_COMPLICATED.digest(),
                DIRECTORY_COMPLICATED.size(),
            )
            .unwrap(),
        ),
        blob_service,
        directory_service,
    )
    .await
    .expect("must succeed");

    assert_eq!(NAR_CONTENTS_COMPLICATED.len() as u64, nar_size);
    let d = Sha256::digest(NAR_CONTENTS_COMPLICATED.clone());
    assert_eq!(d.as_slice(), nar_digest);
}
