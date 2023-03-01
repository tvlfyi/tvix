use super::utils::{gen_blob_service, gen_chunk_service, gen_directory_service};
use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryService;
use crate::import::import_path;
use crate::proto;
use crate::tests::fixtures::DIRECTORY_COMPLICATED;
use crate::tests::fixtures::*;
use tempfile::TempDir;

#[cfg(target_family = "unix")]
#[test]
fn symlink() {
    let tmpdir = TempDir::new().unwrap();

    std::fs::create_dir_all(&tmpdir).unwrap();
    std::os::unix::fs::symlink(
        "/nix/store/somewhereelse",
        tmpdir.path().join("doesntmatter"),
    )
    .unwrap();

    let root_node = import_path(
        &mut gen_blob_service(),
        &mut gen_chunk_service(),
        &mut gen_directory_service(),
        tmpdir.path().join("doesntmatter"),
    )
    .expect("must succeed");

    assert_eq!(
        crate::proto::node::Node::Symlink(proto::SymlinkNode {
            name: "doesntmatter".to_string(),
            target: "/nix/store/somewhereelse".to_string(),
        }),
        root_node,
    )
}

#[test]
fn single_file() {
    let tmpdir = TempDir::new().unwrap();

    std::fs::write(tmpdir.path().join("root"), HELLOWORLD_BLOB_CONTENTS).unwrap();

    let mut blob_service = gen_blob_service();

    let root_node = import_path(
        &mut blob_service,
        &mut gen_chunk_service(),
        &mut gen_directory_service(),
        tmpdir.path().join("root"),
    )
    .expect("must succeed");

    assert_eq!(
        crate::proto::node::Node::File(proto::FileNode {
            name: "root".to_string(),
            digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
            size: HELLOWORLD_BLOB_CONTENTS.len() as u32,
            executable: false,
        }),
        root_node,
    );

    // ensure the blob has been uploaded
    assert!(blob_service
        .stat(&proto::StatBlobRequest {
            digest: HELLOWORLD_BLOB_DIGEST.to_vec(),
            include_chunks: false,
            ..Default::default()
        })
        .unwrap()
        .is_some());
}

#[test]
fn complicated() {
    let tmpdir = TempDir::new().unwrap();

    // File ``.keep`
    std::fs::write(tmpdir.path().join(".keep"), vec![]).unwrap();
    // Symlink `aa`
    std::os::unix::fs::symlink("/nix/store/somewhereelse", tmpdir.path().join("aa")).unwrap();
    // Directory `keep`
    std::fs::create_dir(tmpdir.path().join("keep")).unwrap();
    // File ``keep/.keep`
    std::fs::write(tmpdir.path().join("keep").join(".keep"), vec![]).unwrap();

    let mut blob_service = gen_blob_service();
    let mut directory_service = gen_directory_service();

    let root_node = import_path(
        &mut blob_service,
        &mut gen_chunk_service(),
        &mut directory_service,
        tmpdir.path(),
    )
    .expect("must succeed");

    // ensure root_node matched expectations
    assert_eq!(
        crate::proto::node::Node::Directory(proto::DirectoryNode {
            name: tmpdir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string(),
            digest: DIRECTORY_COMPLICATED.digest(),
            size: DIRECTORY_COMPLICATED.size(),
        }),
        root_node,
    );

    // ensure DIRECTORY_WITH_KEEP and DIRECTORY_COMPLICATED have been uploaded
    assert!(directory_service
        .get(&proto::get_directory_request::ByWhat::Digest(
            DIRECTORY_WITH_KEEP.digest()
        ))
        .unwrap()
        .is_some());
    assert!(directory_service
        .get(&proto::get_directory_request::ByWhat::Digest(
            DIRECTORY_COMPLICATED.digest()
        ))
        .unwrap()
        .is_some());

    // ensure EMPTY_BLOB_CONTENTS has been uploaded
    assert!(blob_service
        .stat(&proto::StatBlobRequest {
            digest: EMPTY_BLOB_DIGEST.to_vec(),
            include_chunks: false,
            include_bao: false
        })
        .unwrap()
        .is_some());
}
