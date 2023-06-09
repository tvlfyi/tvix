use crate::nar::calculate_size_and_sha256;
use crate::nar::write_nar;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::tests::fixtures::*;
use crate::tests::utils::*;
use sha2::{Digest, Sha256};
use std::io;

#[test]
fn single_symlink() {
    let mut buf: Vec<u8> = vec![];

    write_nar(
        &mut buf,
        &crate::proto::node::Node::Symlink(SymlinkNode {
            name: "doesntmatter".to_string(),
            target: "/nix/store/somewhereelse".to_string(),
        }),
        // don't put anything in the stores, as we don't actually do any requests.
        gen_blob_service(),
        gen_directory_service(),
    )
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_SYMLINK.to_vec());
}

/// Make sure the NARRenderer fails if a referred blob doesn't exist.
#[test]
fn single_file_missing_blob() {
    let mut buf: Vec<u8> = vec![];

    let e = write_nar(
        &mut buf,
        &crate::proto::node::Node::File(FileNode {
            name: "doesntmatter".to_string(),
            digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
            size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
            executable: false,
        }),
        // the blobservice is empty intentionally, to provoke the error.
        gen_blob_service(),
        gen_directory_service(),
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
    assert_eq!(HELLOWORLD_BLOB_DIGEST.clone(), writer.close().unwrap());

    // Test with a root FileNode of a too big size
    {
        let mut buf: Vec<u8> = vec![];

        let e = write_nar(
            &mut buf,
            &crate::proto::node::Node::File(FileNode {
                name: "doesntmatter".to_string(),
                digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                size: 42, // <- note the wrong size here!
                executable: false,
            }),
            blob_service.clone(),
            gen_directory_service(),
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

        let e = write_nar(
            &mut buf,
            &crate::proto::node::Node::File(FileNode {
                name: "doesntmatter".to_string(),
                digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
                size: 2, // <- note the wrong size here!
                executable: false,
            }),
            blob_service,
            gen_directory_service(),
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
    assert_eq!(HELLOWORLD_BLOB_DIGEST.clone(), writer.close().unwrap());

    let mut buf: Vec<u8> = vec![];

    write_nar(
        &mut buf,
        &crate::proto::node::Node::File(FileNode {
            name: "doesntmatter".to_string(),
            digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
            size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
            executable: false,
        }),
        blob_service,
        gen_directory_service(),
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
    assert_eq!(EMPTY_BLOB_DIGEST.clone(), writer.close().unwrap());

    directory_service.put(DIRECTORY_WITH_KEEP.clone()).unwrap();
    directory_service
        .put(DIRECTORY_COMPLICATED.clone())
        .unwrap();

    let mut buf: Vec<u8> = vec![];

    write_nar(
        &mut buf,
        &crate::proto::node::Node::Directory(DirectoryNode {
            name: "doesntmatter".to_string(),
            digest: DIRECTORY_COMPLICATED.digest().to_vec(),
            size: DIRECTORY_COMPLICATED.size(),
        }),
        blob_service.clone(),
        directory_service.clone(),
    )
    .expect("must succeed");

    assert_eq!(buf, NAR_CONTENTS_COMPLICATED.to_vec());

    // ensure calculate_nar does return the correct sha256 digest and sum.
    let (nar_size, nar_digest) = calculate_size_and_sha256(
        &crate::proto::node::Node::Directory(DirectoryNode {
            name: "doesntmatter".to_string(),
            digest: DIRECTORY_COMPLICATED.digest().to_vec(),
            size: DIRECTORY_COMPLICATED.size(),
        }),
        blob_service,
        directory_service,
    )
    .expect("must succeed");

    assert_eq!(NAR_CONTENTS_COMPLICATED.len() as u64, nar_size);
    let d = Sha256::digest(NAR_CONTENTS_COMPLICATED.clone());
    assert_eq!(d.as_slice(), nar_digest);
}
