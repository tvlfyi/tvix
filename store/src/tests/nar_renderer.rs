use crate::nar::calculate_size_and_sha256;
use crate::nar::write_nar;
use crate::tests::fixtures::*;
use crate::tests::utils::*;
use sha2::{Digest, Sha256};
use std::io;
use std::sync::Arc;
use tokio::io::sink;
use tvix_castore::blobservice::BlobService;
use tvix_castore::directoryservice::DirectoryService;
use tvix_castore::proto as castorepb;

#[tokio::test]
async fn single_symlink() {
    let blob_service: Arc<dyn BlobService> = gen_blob_service().into();
    let directory_service: Arc<dyn DirectoryService> = gen_directory_service().into();
    let mut buf: Vec<u8> = vec![];

    write_nar(
        &mut buf,
        &castorepb::node::Node::Symlink(castorepb::SymlinkNode {
            name: "doesntmatter".into(),
            target: "/nix/store/somewhereelse".into(),
        }),
        // don't put anything in the stores, as we don't actually do any requests.
        blob_service,
        directory_service,
    )
    .await
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_SYMLINK.to_vec());
}

/// Make sure the NARRenderer fails if a referred blob doesn't exist.
#[tokio::test]
async fn single_file_missing_blob() {
    let blob_service: Arc<dyn BlobService> = gen_blob_service().into();
    let directory_service: Arc<dyn DirectoryService> = gen_directory_service().into();

    let e = write_nar(
        sink(),
        &castorepb::node::Node::File(castorepb::FileNode {
            name: "doesntmatter".into(),
            digest: HELLOWORLD_BLOB_DIGEST.clone().into(),
            size: HELLOWORLD_BLOB_CONTENTS.len() as u64,
            executable: false,
        }),
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
#[tokio::test]
async fn single_file_wrong_blob_size() {
    let blob_service: Arc<dyn BlobService> = gen_blob_service().into();

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

    let bs = blob_service.clone();
    // Test with a root FileNode of a too big size
    {
        let directory_service: Arc<dyn DirectoryService> = gen_directory_service().into();
        let e = write_nar(
            sink(),
            &castorepb::node::Node::File(castorepb::FileNode {
                name: "doesntmatter".into(),
                digest: HELLOWORLD_BLOB_DIGEST.clone().into(),
                size: 42, // <- note the wrong size here!
                executable: false,
            }),
            bs,
            directory_service,
        )
        .await
        .expect_err("must fail");

        match e {
            crate::nar::RenderError::NARWriterError(e) => {
                assert_eq!(io::ErrorKind::UnexpectedEof, e.kind());
            }
            _ => panic!("unexpected error: {:?}", e),
        }
    }

    let bs = blob_service.clone();
    // Test with a root FileNode of a too small size
    {
        let directory_service: Arc<dyn DirectoryService> = gen_directory_service().into();
        let e = write_nar(
            sink(),
            &castorepb::node::Node::File(castorepb::FileNode {
                name: "doesntmatter".into(),
                digest: HELLOWORLD_BLOB_DIGEST.clone().into(),
                size: 2, // <- note the wrong size here!
                executable: false,
            }),
            bs,
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
}

#[tokio::test]
async fn single_file() {
    let blob_service: Arc<dyn BlobService> = gen_blob_service().into();
    let directory_service: Arc<dyn DirectoryService> = gen_directory_service().into();

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
        &castorepb::node::Node::File(castorepb::FileNode {
            name: "doesntmatter".into(),
            digest: HELLOWORLD_BLOB_DIGEST.clone().into(),
            size: HELLOWORLD_BLOB_CONTENTS.len() as u64,
            executable: false,
        }),
        blob_service,
        directory_service,
    )
    .await
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_HELLOWORLD.to_vec());
}

#[tokio::test]
async fn test_complicated() {
    let blob_service: Arc<dyn BlobService> = gen_blob_service().into();
    let directory_service: Arc<dyn DirectoryService> = gen_directory_service().into();

    // put all data into the stores.
    // insert blob into the store
    let mut writer = blob_service.open_write().await;
    tokio::io::copy(&mut io::Cursor::new(EMPTY_BLOB_CONTENTS), &mut writer)
        .await
        .unwrap();
    assert_eq!(EMPTY_BLOB_DIGEST.clone(), writer.close().await.unwrap());

    directory_service
        .put(DIRECTORY_WITH_KEEP.clone())
        .await
        .unwrap();
    directory_service
        .put(DIRECTORY_COMPLICATED.clone())
        .await
        .unwrap();

    let mut buf: Vec<u8> = vec![];

    let bs = blob_service.clone();
    let ds = directory_service.clone();

    write_nar(
        &mut buf,
        &castorepb::node::Node::Directory(castorepb::DirectoryNode {
            name: "doesntmatter".into(),
            digest: DIRECTORY_COMPLICATED.digest().into(),
            size: DIRECTORY_COMPLICATED.size(),
        }),
        bs,
        ds,
    )
    .await
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_COMPLICATED.to_vec());

    // ensure calculate_nar does return the correct sha256 digest and sum.
    let bs = blob_service.clone();
    let ds = directory_service.clone();
    let (nar_size, nar_digest) = calculate_size_and_sha256(
        &castorepb::node::Node::Directory(castorepb::DirectoryNode {
            name: "doesntmatter".into(),
            digest: DIRECTORY_COMPLICATED.digest().into(),
            size: DIRECTORY_COMPLICATED.size(),
        }),
        bs,
        ds,
    )
    .await
    .expect("must succeed");

    assert_eq!(NAR_CONTENTS_COMPLICATED.len() as u64, nar_size);
    let d = Sha256::digest(NAR_CONTENTS_COMPLICATED.clone());
    assert_eq!(d.as_slice(), nar_digest);
}
