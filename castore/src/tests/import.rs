use crate::blobservice::{self, BlobService};
use crate::directoryservice;
use crate::fixtures::*;
use crate::import::ingest_path;
use crate::proto;

use std::sync::Arc;
use tempfile::TempDir;

#[cfg(target_family = "unix")]
use std::os::unix::ffi::OsStrExt;

#[cfg(target_family = "unix")]
#[tokio::test]
async fn symlink() {
    let blob_service = blobservice::from_addr("memory://").await.unwrap();
    let directory_service = directoryservice::from_addr("memory://").await.unwrap();

    let tmpdir = TempDir::new().unwrap();

    std::fs::create_dir_all(&tmpdir).unwrap();
    std::os::unix::fs::symlink(
        "/nix/store/somewhereelse",
        tmpdir.path().join("doesntmatter"),
    )
    .unwrap();

    let root_node = ingest_path(
        Arc::from(blob_service),
        directory_service,
        tmpdir.path().join("doesntmatter"),
    )
    .await
    .expect("must succeed");

    assert_eq!(
        proto::node::Node::Symlink(proto::SymlinkNode {
            name: "doesntmatter".into(),
            target: "/nix/store/somewhereelse".into(),
        }),
        root_node,
    )
}

#[tokio::test]
async fn single_file() {
    let blob_service =
        Arc::from(blobservice::from_addr("memory://").await.unwrap()) as Arc<dyn BlobService>;
    let directory_service = directoryservice::from_addr("memory://").await.unwrap();

    let tmpdir = TempDir::new().unwrap();

    std::fs::write(tmpdir.path().join("root"), HELLOWORLD_BLOB_CONTENTS).unwrap();

    let root_node = ingest_path(
        blob_service.clone(),
        directory_service,
        tmpdir.path().join("root"),
    )
    .await
    .expect("must succeed");

    assert_eq!(
        proto::node::Node::File(proto::FileNode {
            name: "root".into(),
            digest: HELLOWORLD_BLOB_DIGEST.clone().into(),
            size: HELLOWORLD_BLOB_CONTENTS.len() as u64,
            executable: false,
        }),
        root_node,
    );

    // ensure the blob has been uploaded
    assert!(blob_service.has(&HELLOWORLD_BLOB_DIGEST).await.unwrap());
}

#[cfg(target_family = "unix")]
#[tokio::test]
async fn complicated() {
    let blob_service =
        Arc::from(blobservice::from_addr("memory://").await.unwrap()) as Arc<dyn BlobService>;
    let directory_service = directoryservice::from_addr("memory://").await.unwrap();

    let tmpdir = TempDir::new().unwrap();

    // File ``.keep`
    std::fs::write(tmpdir.path().join(".keep"), vec![]).unwrap();
    // Symlink `aa`
    std::os::unix::fs::symlink("/nix/store/somewhereelse", tmpdir.path().join("aa")).unwrap();
    // Directory `keep`
    std::fs::create_dir(tmpdir.path().join("keep")).unwrap();
    // File ``keep/.keep`
    std::fs::write(tmpdir.path().join("keep").join(".keep"), vec![]).unwrap();

    let root_node = ingest_path(blob_service.clone(), &directory_service, tmpdir.path())
        .await
        .expect("must succeed");

    // ensure root_node matched expectations
    assert_eq!(
        proto::node::Node::Directory(proto::DirectoryNode {
            name: tmpdir
                .path()
                .file_name()
                .unwrap()
                .as_bytes()
                .to_owned()
                .into(),
            digest: DIRECTORY_COMPLICATED.digest().into(),
            size: DIRECTORY_COMPLICATED.size(),
        }),
        root_node,
    );

    // ensure DIRECTORY_WITH_KEEP and DIRECTORY_COMPLICATED have been uploaded
    assert!(directory_service
        .get(&DIRECTORY_WITH_KEEP.digest())
        .await
        .unwrap()
        .is_some());
    assert!(directory_service
        .get(&DIRECTORY_COMPLICATED.digest())
        .await
        .unwrap()
        .is_some());

    // ensure EMPTY_BLOB_CONTENTS has been uploaded
    assert!(blob_service.has(&EMPTY_BLOB_DIGEST).await.unwrap());
}
