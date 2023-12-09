use futures::StreamExt;
use std::io::Cursor;
use std::os::unix::prelude::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use tokio::{fs, io};
use tokio_stream::wrappers::ReadDirStream;
use tvix_castore::blobservice::BlobService;
use tvix_castore::directoryservice::DirectoryService;

use tempfile::TempDir;

use crate::fs::{fuse::FuseDaemon, TvixStoreFs};
use crate::pathinfoservice::PathInfoService;
use crate::proto::PathInfo;
use crate::tests::fixtures;
use crate::tests::utils::{gen_blob_service, gen_directory_service, gen_pathinfo_service};
use tvix_castore::proto as castorepb;

const BLOB_A_NAME: &str = "00000000000000000000000000000000-test";
const BLOB_B_NAME: &str = "55555555555555555555555555555555-test";
const HELLOWORLD_BLOB_NAME: &str = "66666666666666666666666666666666-test";
const SYMLINK_NAME: &str = "11111111111111111111111111111111-test";
const SYMLINK_NAME2: &str = "44444444444444444444444444444444-test";
const DIRECTORY_WITH_KEEP_NAME: &str = "22222222222222222222222222222222-test";
const DIRECTORY_COMPLICATED_NAME: &str = "33333333333333333333333333333333-test";

fn gen_svcs() -> (
    Arc<dyn BlobService>,
    Arc<dyn DirectoryService>,
    Arc<dyn PathInfoService>,
) {
    let blob_service = gen_blob_service();
    let directory_service = gen_directory_service();
    let path_info_service = gen_pathinfo_service(blob_service.clone(), directory_service.clone());

    (blob_service, directory_service, path_info_service)
}

fn do_mount<P: AsRef<Path>>(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,
    mountpoint: P,
    list_root: bool,
) -> io::Result<FuseDaemon> {
    let fs = TvixStoreFs::new(
        blob_service,
        directory_service,
        path_info_service,
        list_root,
    );
    FuseDaemon::new(fs, mountpoint.as_ref(), 4)
}

async fn populate_blob_a(
    blob_service: &Arc<dyn BlobService>,
    _directory_service: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // Upload BLOB_A
    let mut bw = blob_service.open_write().await;
    tokio::io::copy(&mut Cursor::new(fixtures::BLOB_A.to_vec()), &mut bw)
        .await
        .expect("must succeed uploading");
    bw.close().await.expect("must succeed closing");

    // Create a PathInfo for it
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::File(castorepb::FileNode {
                name: BLOB_A_NAME.into(),
                digest: fixtures::BLOB_A_DIGEST.clone().into(),
                size: fixtures::BLOB_A.len() as u64,
                executable: false,
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

async fn populate_blob_b(
    blob_service: &Arc<dyn BlobService>,
    _directory_service: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // Upload BLOB_B
    let mut bw = blob_service.open_write().await;
    tokio::io::copy(&mut Cursor::new(fixtures::BLOB_B.to_vec()), &mut bw)
        .await
        .expect("must succeed uploading");
    bw.close().await.expect("must succeed closing");

    // Create a PathInfo for it
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::File(castorepb::FileNode {
                name: BLOB_B_NAME.into(),
                digest: fixtures::BLOB_B_DIGEST.clone().into(),
                size: fixtures::BLOB_B.len() as u64,
                executable: false,
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

/// adds a blob containing helloworld and marks it as executable
async fn populate_helloworld_blob(
    blob_service: &Arc<dyn BlobService>,
    _directory_service: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // Upload BLOB_B
    let mut bw = blob_service.open_write().await;
    tokio::io::copy(
        &mut Cursor::new(fixtures::HELLOWORLD_BLOB_CONTENTS.to_vec()),
        &mut bw,
    )
    .await
    .expect("must succeed uploading");
    bw.close().await.expect("must succeed closing");

    // Create a PathInfo for it
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::File(castorepb::FileNode {
                name: HELLOWORLD_BLOB_NAME.into(),
                digest: fixtures::HELLOWORLD_BLOB_DIGEST.clone().into(),
                size: fixtures::HELLOWORLD_BLOB_CONTENTS.len() as u64,
                executable: true,
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

async fn populate_symlink(
    _blob_service: &Arc<dyn BlobService>,
    _directory_service: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // Create a PathInfo for it
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Symlink(castorepb::SymlinkNode {
                name: SYMLINK_NAME.into(),
                target: BLOB_A_NAME.into(),
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

/// This writes a symlink pointing to /nix/store/somewhereelse,
/// which is the same symlink target as "aa" inside DIRECTORY_COMPLICATED.
async fn populate_symlink2(
    _blob_service: &Arc<dyn BlobService>,
    _directory_service: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // Create a PathInfo for it
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Symlink(castorepb::SymlinkNode {
                name: SYMLINK_NAME2.into(),
                target: "/nix/store/somewhereelse".into(),
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

async fn populate_directory_with_keep(
    blob_service: &Arc<dyn BlobService>,
    directory_service: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // upload empty blob
    let mut bw = blob_service.open_write().await;
    assert_eq!(
        fixtures::EMPTY_BLOB_DIGEST.as_slice(),
        bw.close().await.expect("must succeed closing").as_slice(),
    );

    // upload directory
    directory_service
        .put(fixtures::DIRECTORY_WITH_KEEP.clone())
        .await
        .expect("must succeed uploading");

    // upload pathinfo
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Directory(castorepb::DirectoryNode {
                name: DIRECTORY_WITH_KEEP_NAME.into(),
                digest: fixtures::DIRECTORY_WITH_KEEP.digest().into(),
                size: fixtures::DIRECTORY_WITH_KEEP.size(),
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

/// Insert [PathInfo] for DIRECTORY_WITH_KEEP, but don't provide the Directory
/// itself.
async fn populate_pathinfo_without_directory(
    _: &Arc<dyn BlobService>,
    _: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // upload pathinfo
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Directory(castorepb::DirectoryNode {
                name: DIRECTORY_WITH_KEEP_NAME.into(),
                digest: fixtures::DIRECTORY_WITH_KEEP.digest().into(),
                size: fixtures::DIRECTORY_WITH_KEEP.size(),
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

/// Insert , but don't provide the blob .keep is pointing to
async fn populate_blob_a_without_blob(
    _: &Arc<dyn BlobService>,
    _: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // Create a PathInfo for blob A
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::File(castorepb::FileNode {
                name: BLOB_A_NAME.into(),
                digest: fixtures::BLOB_A_DIGEST.clone().into(),
                size: fixtures::BLOB_A.len() as u64,
                executable: false,
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

async fn populate_directory_complicated(
    blob_service: &Arc<dyn BlobService>,
    directory_service: &Arc<dyn DirectoryService>,
    path_info_service: &Arc<dyn PathInfoService>,
) {
    // upload empty blob
    let mut bw = blob_service.open_write().await;
    assert_eq!(
        fixtures::EMPTY_BLOB_DIGEST.as_slice(),
        bw.close().await.expect("must succeed closing").as_slice(),
    );

    // upload inner directory
    directory_service
        .put(fixtures::DIRECTORY_WITH_KEEP.clone())
        .await
        .expect("must succeed uploading");

    // uplodad parent directory
    directory_service
        .put(fixtures::DIRECTORY_COMPLICATED.clone())
        .await
        .expect("must succeed uploading");

    // upload pathinfo
    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Directory(castorepb::DirectoryNode {
                name: DIRECTORY_COMPLICATED_NAME.into(),
                digest: fixtures::DIRECTORY_COMPLICATED.digest().into(),
                size: fixtures::DIRECTORY_COMPLICATED.size(),
            })),
        }),
        ..Default::default()
    };
    path_info_service
        .put(path_info)
        .await
        .expect("must succeed");
}

/// Ensure mounting itself doesn't fail
#[tokio::test]
async fn mount() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }

    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure listing the root isn't allowed
#[tokio::test]
async fn root() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    {
        // read_dir succeeds, but getting the first element will fail.
        let mut it = ReadDirStream::new(fs::read_dir(tmpdir).await.expect("must succeed"));

        let err = it
            .next()
            .await
            .expect("must be some")
            .expect_err("must be err");
        assert_eq!(std::io::ErrorKind::PermissionDenied, err.kind());
    }

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure listing the root is allowed if configured explicitly
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn root_with_listing() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_a(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        true, /* allow listing */
    )
    .expect("must succeed");

    {
        // read_dir succeeds, but getting the first element will fail.
        let mut it = ReadDirStream::new(fs::read_dir(tmpdir).await.expect("must succeed"));

        let e = it
            .next()
            .await
            .expect("must be some")
            .expect("must succeed");

        let metadata = e.metadata().await.expect("must succeed");
        assert!(metadata.is_file());
        assert!(metadata.permissions().readonly());
        assert_eq!(fixtures::BLOB_A.len() as u64, metadata.len());
    }

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure we can stat a file at the root
#[tokio::test]
async fn stat_file_at_root() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_a(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);

    // peek at the file metadata
    let metadata = fs::metadata(p).await.expect("must succeed");

    assert!(metadata.is_file());
    assert!(metadata.permissions().readonly());
    assert_eq!(fixtures::BLOB_A.len() as u64, metadata.len());

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure we can read a file at the root
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_file_at_root() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_a(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);

    // read the file contents
    let data = fs::read(p).await.expect("must succeed");

    // ensure size and contents match
    assert_eq!(fixtures::BLOB_A.len(), data.len());
    assert_eq!(fixtures::BLOB_A.to_vec(), data);

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure we can read a large file at the root
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_large_file_at_root() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_b(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_B_NAME);
    {
        // peek at the file metadata
        let metadata = fs::metadata(&p).await.expect("must succeed");

        assert!(metadata.is_file());
        assert!(metadata.permissions().readonly());
        assert_eq!(fixtures::BLOB_B.len() as u64, metadata.len());
    }

    // read the file contents
    let data = fs::read(p).await.expect("must succeed");

    // ensure size and contents match
    assert_eq!(fixtures::BLOB_B.len(), data.len());
    assert_eq!(fixtures::BLOB_B.to_vec(), data);

    fuse_daemon.unmount().expect("unmount");
}

/// Read the target of a symlink
#[tokio::test]
async fn symlink_readlink() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_symlink(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(SYMLINK_NAME);

    let target = fs::read_link(&p).await.expect("must succeed");
    assert_eq!(BLOB_A_NAME, target.to_str().unwrap());

    // peek at the file metadata, which follows symlinks.
    // this must fail, as we didn't populate the target.
    let e = fs::metadata(&p).await.expect_err("must fail");
    assert_eq!(std::io::ErrorKind::NotFound, e.kind());

    // peeking at the file metadata without following symlinks will succeed.
    let metadata = fs::symlink_metadata(&p).await.expect("must succeed");
    assert!(metadata.is_symlink());

    // reading from the symlink (which follows) will fail, because the target doesn't exist.
    let e = fs::read(p).await.expect_err("must fail");
    assert_eq!(std::io::ErrorKind::NotFound, e.kind());

    fuse_daemon.unmount().expect("unmount");
}

/// Read and stat a regular file through a symlink pointing to it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_stat_through_symlink() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_a(&blob_service, &directory_service, &path_info_service).await;
    populate_symlink(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p_symlink = tmpdir.path().join(SYMLINK_NAME);
    let p_blob = tmpdir.path().join(SYMLINK_NAME);

    // peek at the file metadata, which follows symlinks.
    // this must now return the same metadata as when statting at the target directly.
    let metadata_symlink = fs::metadata(&p_symlink).await.expect("must succeed");
    let metadata_blob = fs::metadata(&p_blob).await.expect("must succeed");
    assert_eq!(metadata_blob.file_type(), metadata_symlink.file_type());
    assert_eq!(metadata_blob.len(), metadata_symlink.len());

    // reading from the symlink (which follows) will return the same data as if
    // we were reading from the file directly.
    assert_eq!(
        fs::read(p_blob).await.expect("must succeed"),
        fs::read(p_symlink).await.expect("must succeed"),
    );

    fuse_daemon.unmount().expect("unmount");
}

/// Read a directory in the root, and validate some attributes.
#[tokio::test]
async fn read_stat_directory() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_with_keep(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);

    // peek at the metadata of the directory
    let metadata = fs::metadata(p).await.expect("must succeed");
    assert!(metadata.is_dir());
    assert!(metadata.permissions().readonly());

    fuse_daemon.unmount().expect("unmount");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// Read a blob inside a directory. This ensures we successfully populate directory data.
async fn read_blob_inside_dir() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_with_keep(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME).join(".keep");

    // peek at metadata.
    let metadata = fs::metadata(&p).await.expect("must succeed");
    assert!(metadata.is_file());
    assert!(metadata.permissions().readonly());

    // read from it
    let data = fs::read(&p).await.expect("must succeed");
    assert_eq!(fixtures::EMPTY_BLOB_CONTENTS.to_vec(), data);

    fuse_daemon.unmount().expect("unmount");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// Read a blob inside a directory inside a directory. This ensures we properly
/// populate directories as we traverse down the structure.
async fn read_blob_deep_inside_dir() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_complicated(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir
        .path()
        .join(DIRECTORY_COMPLICATED_NAME)
        .join("keep")
        .join(".keep");

    // peek at metadata.
    let metadata = fs::metadata(&p).await.expect("must succeed");
    assert!(metadata.is_file());
    assert!(metadata.permissions().readonly());

    // read from it
    let data = fs::read(&p).await.expect("must succeed");
    assert_eq!(fixtures::EMPTY_BLOB_CONTENTS.to_vec(), data);

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure readdir works.
#[tokio::test]
async fn readdir() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_complicated(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME);

    {
        // read_dir should succeed. Collect all elements
        let elements: Vec<_> = ReadDirStream::new(fs::read_dir(p).await.expect("must succeed"))
            .map(|e| e.expect("must not be err"))
            .collect()
            .await;

        assert_eq!(3, elements.len(), "number of elements should be 3"); // rust skips . and ..

        // We explicitly look at specific positions here, because we always emit
        // them ordered.

        // ".keep", 0 byte file.
        let e = &elements[0];
        assert_eq!(".keep", e.file_name());
        assert!(e.file_type().await.expect("must succeed").is_file());
        assert_eq!(0, e.metadata().await.expect("must succeed").len());

        // "aa", symlink.
        let e = &elements[1];
        assert_eq!("aa", e.file_name());
        assert!(e.file_type().await.expect("must succeed").is_symlink());

        // "keep", directory
        let e = &elements[2];
        assert_eq!("keep", e.file_name());
        assert!(e.file_type().await.expect("must succeed").is_dir());
    }

    fuse_daemon.unmount().expect("unmount");
}

#[tokio::test]
/// Do a readdir deeper inside a directory, without doing readdir or stat in the parent directory.
async fn readdir_deep() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_complicated(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME).join("keep");

    {
        // read_dir should succeed. Collect all elements
        let elements: Vec<_> = ReadDirStream::new(fs::read_dir(p).await.expect("must succeed"))
            .map(|e| e.expect("must not be err"))
            .collect()
            .await;

        assert_eq!(1, elements.len(), "number of elements should be 1"); // rust skips . and ..

        // ".keep", 0 byte file.
        let e = &elements[0];
        assert_eq!(".keep", e.file_name());
        assert!(e.file_type().await.expect("must succeed").is_file());
        assert_eq!(0, e.metadata().await.expect("must succeed").len());
    }

    fuse_daemon.unmount().expect("unmount");
}

/// Check attributes match how they show up in /nix/store normally.
#[tokio::test]
async fn check_attributes() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_a(&blob_service, &directory_service, &path_info_service).await;
    populate_directory_with_keep(&blob_service, &directory_service, &path_info_service).await;
    populate_symlink(&blob_service, &directory_service, &path_info_service).await;
    populate_helloworld_blob(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p_file = tmpdir.path().join(BLOB_A_NAME);
    let p_directory = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);
    let p_symlink = tmpdir.path().join(SYMLINK_NAME);
    let p_executable_file = tmpdir.path().join(HELLOWORLD_BLOB_NAME);

    // peek at metadata. We use symlink_metadata to ensure we don't traverse a symlink by accident.
    let metadata_file = fs::symlink_metadata(&p_file).await.expect("must succeed");
    let metadata_executable_file = fs::symlink_metadata(&p_executable_file)
        .await
        .expect("must succeed");
    let metadata_directory = fs::symlink_metadata(&p_directory)
        .await
        .expect("must succeed");
    let metadata_symlink = fs::symlink_metadata(&p_symlink)
        .await
        .expect("must succeed");

    // modes should match. We & with 0o777 to remove any higher bits.
    assert_eq!(0o444, metadata_file.mode() & 0o777);
    assert_eq!(0o555, metadata_executable_file.mode() & 0o777);
    assert_eq!(0o555, metadata_directory.mode() & 0o777);
    assert_eq!(0o444, metadata_symlink.mode() & 0o777);

    // files should have the correct filesize
    assert_eq!(fixtures::BLOB_A.len() as u64, metadata_file.len());
    // directories should have their "size" as filesize
    assert_eq!(
        { fixtures::DIRECTORY_WITH_KEEP.size() },
        metadata_directory.size()
    );

    for metadata in &[&metadata_file, &metadata_directory, &metadata_symlink] {
        // uid and gid should be 0.
        assert_eq!(0, metadata.uid());
        assert_eq!(0, metadata.gid());

        // all times should be set to the unix epoch.
        assert_eq!(0, metadata.atime());
        assert_eq!(0, metadata.mtime());
        assert_eq!(0, metadata.ctime());
        // crtime seems MacOS only
    }

    fuse_daemon.unmount().expect("unmount");
}

#[tokio::test]
/// Ensure we allocate the same inodes for the same directory contents.
/// $DIRECTORY_COMPLICATED_NAME/keep contains the same data as $DIRECTORY_WITH_KEEP.
async fn compare_inodes_directories() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_with_keep(&blob_service, &directory_service, &path_info_service).await;
    populate_directory_complicated(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p_dir_with_keep = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);
    let p_sibling_dir = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME).join("keep");

    // peek at metadata.
    assert_eq!(
        fs::metadata(p_dir_with_keep)
            .await
            .expect("must succeed")
            .ino(),
        fs::metadata(p_sibling_dir)
            .await
            .expect("must succeed")
            .ino()
    );

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure we allocate the same inodes for the same directory contents.
/// $DIRECTORY_COMPLICATED_NAME/keep/,keep contains the same data as $DIRECTORY_COMPLICATED_NAME/.keep
#[tokio::test]
async fn compare_inodes_files() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_complicated(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p_keep1 = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME).join(".keep");
    let p_keep2 = tmpdir
        .path()
        .join(DIRECTORY_COMPLICATED_NAME)
        .join("keep")
        .join(".keep");

    // peek at metadata.
    assert_eq!(
        fs::metadata(p_keep1).await.expect("must succeed").ino(),
        fs::metadata(p_keep2).await.expect("must succeed").ino()
    );

    fuse_daemon.unmount().expect("unmount");
}

/// Ensure we allocate the same inode for symlinks pointing to the same targets.
/// $DIRECTORY_COMPLICATED_NAME/aa points to the same target as SYMLINK_NAME2.
#[tokio::test]
async fn compare_inodes_symlinks() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_directory_complicated(&blob_service, &directory_service, &path_info_service).await;
    populate_symlink2(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p1 = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME).join("aa");
    let p2 = tmpdir.path().join(SYMLINK_NAME2);

    // peek at metadata.
    assert_eq!(
        fs::symlink_metadata(p1).await.expect("must succeed").ino(),
        fs::symlink_metadata(p2).await.expect("must succeed").ino()
    );

    fuse_daemon.unmount().expect("unmount");
}

/// Check we match paths exactly.
#[tokio::test]
async fn read_wrong_paths_in_root() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_a(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    // wrong name
    assert!(
        fs::metadata(tmpdir.path().join("00000000000000000000000000000000-tes"))
            .await
            .is_err()
    );

    // invalid hash
    assert!(
        fs::metadata(tmpdir.path().join("0000000000000000000000000000000-test"))
            .await
            .is_err()
    );

    // right name, must exist
    assert!(
        fs::metadata(tmpdir.path().join("00000000000000000000000000000000-test"))
            .await
            .is_ok()
    );

    // now wrong name with right hash still may not exist
    assert!(
        fs::metadata(tmpdir.path().join("00000000000000000000000000000000-tes"))
            .await
            .is_err()
    );

    fuse_daemon.unmount().expect("unmount");
}

/// Make sure writes are not allowed
#[tokio::test]
async fn disallow_writes() {
    // https://plume.benboeckel.net/~/JustAnotherBlog/skipping-tests-in-rust
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }

    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);
    let e = fs::File::create(p).await.expect_err("must fail");

    assert_eq!(Some(libc::EROFS), e.raw_os_error());

    fuse_daemon.unmount().expect("unmount");
}

#[tokio::test]
/// Ensure we get an IO error if the directory service does not have the Directory object.
async fn missing_directory() {
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_pathinfo_without_directory(&blob_service, &directory_service, &path_info_service)
        .await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);

    {
        // `stat` on the path should succeed, because it doesn't trigger the directory request.
        fs::metadata(&p).await.expect("must succeed");

        // However, calling either `readdir` or `stat` on a child should fail with an IO error.
        // It fails when trying to pull the first entry, because we don't implement opendir separately
        ReadDirStream::new(fs::read_dir(&p).await.unwrap())
            .next()
            .await
            .expect("must be some")
            .expect_err("must be err");

        // rust currently sets e.kind() to Uncategorized, which isn't very
        // helpful, so we don't look at the error more closely than that..
        fs::metadata(p.join(".keep")).await.expect_err("must fail");
    }

    fuse_daemon.unmount().expect("unmount");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// Ensure we get an IO error if the blob service does not have the blob
async fn missing_blob() {
    if !std::path::Path::new("/dev/fuse").exists() {
        eprintln!("skipping test");
        return;
    }
    let tmpdir = TempDir::new().unwrap();

    let (blob_service, directory_service, path_info_service) = gen_svcs();
    populate_blob_a_without_blob(&blob_service, &directory_service, &path_info_service).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        path_info_service,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);

    {
        // `stat` on the blob should succeed, because it doesn't trigger a request to the blob service.
        fs::metadata(&p).await.expect("must succeed");

        // However, calling read on the blob should fail.
        // rust currently sets e.kind() to Uncategorized, which isn't very
        // helpful, so we don't look at the error more closely than that..
        fs::read(p).await.expect_err("must fail");
    }

    fuse_daemon.unmount().expect("unmount");
}
