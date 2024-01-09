use std::{
    collections::BTreeMap,
    io::{self, Cursor},
    os::unix::fs::MetadataExt,
    path::Path,
    sync::Arc,
};

use bytes::Bytes;
use tempfile::TempDir;
use tokio_stream::{wrappers::ReadDirStream, StreamExt};

use super::{fuse::FuseDaemon, TvixStoreFs};
use crate::proto::node::Node;
use crate::proto::{self as castorepb};
use crate::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    fixtures,
};

const BLOB_A_NAME: &str = "00000000000000000000000000000000-test";
const BLOB_B_NAME: &str = "55555555555555555555555555555555-test";
const HELLOWORLD_BLOB_NAME: &str = "66666666666666666666666666666666-test";
const SYMLINK_NAME: &str = "11111111111111111111111111111111-test";
const SYMLINK_NAME2: &str = "44444444444444444444444444444444-test";
const DIRECTORY_WITH_KEEP_NAME: &str = "22222222222222222222222222222222-test";
const DIRECTORY_COMPLICATED_NAME: &str = "33333333333333333333333333333333-test";

fn gen_svcs() -> (Arc<dyn BlobService>, Arc<dyn DirectoryService>) {
    (
        Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>,
        Arc::new(MemoryDirectoryService::default()) as Arc<dyn DirectoryService>,
    )
}

fn do_mount<P: AsRef<Path>, BS, DS>(
    blob_service: BS,
    directory_service: DS,
    root_nodes: BTreeMap<bytes::Bytes, Node>,
    mountpoint: P,
    list_root: bool,
) -> io::Result<FuseDaemon>
where
    BS: AsRef<dyn BlobService> + Send + Sync + Clone + 'static,
    DS: AsRef<dyn DirectoryService> + Send + Sync + Clone + 'static,
{
    let fs = TvixStoreFs::new(
        blob_service,
        directory_service,
        Arc::new(root_nodes),
        list_root,
    );
    FuseDaemon::new(Arc::new(fs), mountpoint.as_ref(), 4)
}

async fn populate_blob_a(
    blob_service: &Arc<dyn BlobService>,
    root_nodes: &mut BTreeMap<Bytes, Node>,
) {
    let mut bw = blob_service.open_write().await;
    tokio::io::copy(&mut Cursor::new(fixtures::BLOB_A.to_vec()), &mut bw)
        .await
        .expect("must succeed uploading");
    bw.close().await.expect("must succeed closing");

    root_nodes.insert(
        BLOB_A_NAME.into(),
        Node::File(castorepb::FileNode {
            name: BLOB_A_NAME.into(),
            digest: fixtures::BLOB_A_DIGEST.clone().into(),
            size: fixtures::BLOB_A.len() as u64,
            executable: false,
        }),
    );
}

async fn populate_blob_b(
    blob_service: &Arc<dyn BlobService>,
    root_nodes: &mut BTreeMap<Bytes, Node>,
) {
    let mut bw = blob_service.open_write().await;
    tokio::io::copy(&mut Cursor::new(fixtures::BLOB_B.to_vec()), &mut bw)
        .await
        .expect("must succeed uploading");
    bw.close().await.expect("must succeed closing");

    root_nodes.insert(
        BLOB_B_NAME.into(),
        Node::File(castorepb::FileNode {
            name: BLOB_B_NAME.into(),
            digest: fixtures::BLOB_B_DIGEST.clone().into(),
            size: fixtures::BLOB_B.len() as u64,
            executable: false,
        }),
    );
}

/// adds a blob containing helloworld and marks it as executable
async fn populate_blob_helloworld(
    blob_service: &Arc<dyn BlobService>,
    root_nodes: &mut BTreeMap<Bytes, Node>,
) {
    let mut bw = blob_service.open_write().await;
    tokio::io::copy(
        &mut Cursor::new(fixtures::HELLOWORLD_BLOB_CONTENTS.to_vec()),
        &mut bw,
    )
    .await
    .expect("must succeed uploading");
    bw.close().await.expect("must succeed closing");

    root_nodes.insert(
        HELLOWORLD_BLOB_NAME.into(),
        Node::File(castorepb::FileNode {
            name: HELLOWORLD_BLOB_NAME.into(),
            digest: fixtures::HELLOWORLD_BLOB_DIGEST.clone().into(),
            size: fixtures::HELLOWORLD_BLOB_CONTENTS.len() as u64,
            executable: true,
        }),
    );
}

async fn populate_symlink(root_nodes: &mut BTreeMap<Bytes, Node>) {
    root_nodes.insert(
        SYMLINK_NAME.into(),
        Node::Symlink(castorepb::SymlinkNode {
            name: SYMLINK_NAME.into(),
            target: BLOB_A_NAME.into(),
        }),
    );
}

/// This writes a symlink pointing to /nix/store/somewhereelse,
/// which is the same symlink target as "aa" inside DIRECTORY_COMPLICATED.
async fn populate_symlink2(root_nodes: &mut BTreeMap<Bytes, Node>) {
    root_nodes.insert(
        SYMLINK_NAME2.into(),
        Node::Symlink(castorepb::SymlinkNode {
            name: SYMLINK_NAME2.into(),
            target: "/nix/store/somewhereelse".into(),
        }),
    );
}

async fn populate_directory_with_keep(
    blob_service: &Arc<dyn BlobService>,
    directory_service: &Arc<dyn DirectoryService>,
    root_nodes: &mut BTreeMap<Bytes, Node>,
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

    root_nodes.insert(
        DIRECTORY_WITH_KEEP_NAME.into(),
        castorepb::node::Node::Directory(castorepb::DirectoryNode {
            name: DIRECTORY_WITH_KEEP_NAME.into(),
            digest: fixtures::DIRECTORY_WITH_KEEP.digest().into(),
            size: fixtures::DIRECTORY_WITH_KEEP.size(),
        }),
    );
}

/// Create a root node for DIRECTORY_WITH_KEEP, but don't upload the Directory
/// itself.
async fn populate_directorynode_without_directory(root_nodes: &mut BTreeMap<Bytes, Node>) {
    root_nodes.insert(
        DIRECTORY_WITH_KEEP_NAME.into(),
        castorepb::node::Node::Directory(castorepb::DirectoryNode {
            name: DIRECTORY_WITH_KEEP_NAME.into(),
            digest: fixtures::DIRECTORY_WITH_KEEP.digest().into(),
            size: fixtures::DIRECTORY_WITH_KEEP.size(),
        }),
    );
}

/// Insert BLOB_A, but don't provide the blob .keep is pointing to.
async fn populate_filenode_without_blob(root_nodes: &mut BTreeMap<Bytes, Node>) {
    root_nodes.insert(
        BLOB_A_NAME.into(),
        Node::File(castorepb::FileNode {
            name: BLOB_A_NAME.into(),
            digest: fixtures::BLOB_A_DIGEST.clone().into(),
            size: fixtures::BLOB_A.len() as u64,
            executable: false,
        }),
    );
}

async fn populate_directory_complicated(
    blob_service: &Arc<dyn BlobService>,
    directory_service: &Arc<dyn DirectoryService>,
    root_nodes: &mut BTreeMap<Bytes, Node>,
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

    // upload parent directory
    directory_service
        .put(fixtures::DIRECTORY_COMPLICATED.clone())
        .await
        .expect("must succeed uploading");

    root_nodes.insert(
        DIRECTORY_COMPLICATED_NAME.into(),
        Node::Directory(castorepb::DirectoryNode {
            name: DIRECTORY_COMPLICATED_NAME.into(),
            digest: fixtures::DIRECTORY_COMPLICATED.digest().into(),
            size: fixtures::DIRECTORY_COMPLICATED.size(),
        }),
    );
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

    let (blob_service, directory_service) = gen_svcs();

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        BTreeMap::default(),
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

    let (blob_service, directory_service) = gen_svcs();
    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        BTreeMap::default(),
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    {
        // read_dir succeeds, but getting the first element will fail.
        let mut it = ReadDirStream::new(tokio::fs::read_dir(tmpdir).await.expect("must succeed"));

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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_blob_a(&blob_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        true, /* allow listing */
    )
    .expect("must succeed");

    {
        // read_dir succeeds, but getting the first element will fail.
        let mut it = ReadDirStream::new(tokio::fs::read_dir(tmpdir).await.expect("must succeed"));

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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_blob_a(&blob_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);

    // peek at the file metadata
    let metadata = tokio::fs::metadata(p).await.expect("must succeed");

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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_blob_a(&blob_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);

    // read the file contents
    let data = tokio::fs::read(p).await.expect("must succeed");

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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_blob_b(&blob_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_B_NAME);
    {
        // peek at the file metadata
        let metadata = tokio::fs::metadata(&p).await.expect("must succeed");

        assert!(metadata.is_file());
        assert!(metadata.permissions().readonly());
        assert_eq!(fixtures::BLOB_B.len() as u64, metadata.len());
    }

    // read the file contents
    let data = tokio::fs::read(p).await.expect("must succeed");

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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_symlink(&mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(SYMLINK_NAME);

    let target = tokio::fs::read_link(&p).await.expect("must succeed");
    assert_eq!(BLOB_A_NAME, target.to_str().unwrap());

    // peek at the file metadata, which follows symlinks.
    // this must fail, as we didn't populate the target.
    let e = tokio::fs::metadata(&p).await.expect_err("must fail");
    assert_eq!(std::io::ErrorKind::NotFound, e.kind());

    // peeking at the file metadata without following symlinks will succeed.
    let metadata = tokio::fs::symlink_metadata(&p).await.expect("must succeed");
    assert!(metadata.is_symlink());

    // reading from the symlink (which follows) will fail, because the target doesn't exist.
    let e = tokio::fs::read(p).await.expect_err("must fail");
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_blob_a(&blob_service, &mut root_nodes).await;
    populate_symlink(&mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p_symlink = tmpdir.path().join(SYMLINK_NAME);
    let p_blob = tmpdir.path().join(SYMLINK_NAME);

    // peek at the file metadata, which follows symlinks.
    // this must now return the same metadata as when statting at the target directly.
    let metadata_symlink = tokio::fs::metadata(&p_symlink).await.expect("must succeed");
    let metadata_blob = tokio::fs::metadata(&p_blob).await.expect("must succeed");
    assert_eq!(metadata_blob.file_type(), metadata_symlink.file_type());
    assert_eq!(metadata_blob.len(), metadata_symlink.len());

    // reading from the symlink (which follows) will return the same data as if
    // we were reading from the file directly.
    assert_eq!(
        tokio::fs::read(p_blob).await.expect("must succeed"),
        tokio::fs::read(p_symlink).await.expect("must succeed"),
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_with_keep(&blob_service, &directory_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);

    // peek at the metadata of the directory
    let metadata = tokio::fs::metadata(p).await.expect("must succeed");
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_with_keep(&blob_service, &directory_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME).join(".keep");

    // peek at metadata.
    let metadata = tokio::fs::metadata(&p).await.expect("must succeed");
    assert!(metadata.is_file());
    assert!(metadata.permissions().readonly());

    // read from it
    let data = tokio::fs::read(&p).await.expect("must succeed");
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_complicated(&blob_service, &directory_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
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
    let metadata = tokio::fs::metadata(&p).await.expect("must succeed");
    assert!(metadata.is_file());
    assert!(metadata.permissions().readonly());

    // read from it
    let data = tokio::fs::read(&p).await.expect("must succeed");
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_complicated(&blob_service, &directory_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME);

    {
        // read_dir should succeed. Collect all elements
        let elements: Vec<_> =
            ReadDirStream::new(tokio::fs::read_dir(p).await.expect("must succeed"))
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_complicated(&blob_service, &directory_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME).join("keep");

    {
        // read_dir should succeed. Collect all elements
        let elements: Vec<_> =
            ReadDirStream::new(tokio::fs::read_dir(p).await.expect("must succeed"))
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_blob_a(&blob_service, &mut root_nodes).await;
    populate_directory_with_keep(&blob_service, &directory_service, &mut root_nodes).await;
    populate_symlink(&mut root_nodes).await;
    populate_blob_helloworld(&blob_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p_file = tmpdir.path().join(BLOB_A_NAME);
    let p_directory = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);
    let p_symlink = tmpdir.path().join(SYMLINK_NAME);
    let p_executable_file = tmpdir.path().join(HELLOWORLD_BLOB_NAME);

    // peek at metadata. We use symlink_metadata to ensure we don't traverse a symlink by accident.
    let metadata_file = tokio::fs::symlink_metadata(&p_file)
        .await
        .expect("must succeed");
    let metadata_executable_file = tokio::fs::symlink_metadata(&p_executable_file)
        .await
        .expect("must succeed");
    let metadata_directory = tokio::fs::symlink_metadata(&p_directory)
        .await
        .expect("must succeed");
    let metadata_symlink = tokio::fs::symlink_metadata(&p_symlink)
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_with_keep(&blob_service, &directory_service, &mut root_nodes).await;
    populate_directory_complicated(&blob_service, &directory_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p_dir_with_keep = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);
    let p_sibling_dir = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME).join("keep");

    // peek at metadata.
    assert_eq!(
        tokio::fs::metadata(p_dir_with_keep)
            .await
            .expect("must succeed")
            .ino(),
        tokio::fs::metadata(p_sibling_dir)
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_complicated(&blob_service, &directory_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
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
        tokio::fs::metadata(p_keep1)
            .await
            .expect("must succeed")
            .ino(),
        tokio::fs::metadata(p_keep2)
            .await
            .expect("must succeed")
            .ino()
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directory_complicated(&blob_service, &directory_service, &mut root_nodes).await;
    populate_symlink2(&mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p1 = tmpdir.path().join(DIRECTORY_COMPLICATED_NAME).join("aa");
    let p2 = tmpdir.path().join(SYMLINK_NAME2);

    // peek at metadata.
    assert_eq!(
        tokio::fs::symlink_metadata(p1)
            .await
            .expect("must succeed")
            .ino(),
        tokio::fs::symlink_metadata(p2)
            .await
            .expect("must succeed")
            .ino()
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_blob_a(&blob_service, &mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    // wrong name
    assert!(
        tokio::fs::metadata(tmpdir.path().join("00000000000000000000000000000000-tes"))
            .await
            .is_err()
    );

    // invalid hash
    assert!(
        tokio::fs::metadata(tmpdir.path().join("0000000000000000000000000000000-test"))
            .await
            .is_err()
    );

    // right name, must exist
    assert!(
        tokio::fs::metadata(tmpdir.path().join("00000000000000000000000000000000-test"))
            .await
            .is_ok()
    );

    // now wrong name with right hash still may not exist
    assert!(
        tokio::fs::metadata(tmpdir.path().join("00000000000000000000000000000000-tes"))
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

    let (blob_service, directory_service) = gen_svcs();
    let root_nodes = BTreeMap::default();

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);
    let e = tokio::fs::File::create(p).await.expect_err("must fail");

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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_directorynode_without_directory(&mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(DIRECTORY_WITH_KEEP_NAME);

    {
        // `stat` on the path should succeed, because it doesn't trigger the directory request.
        tokio::fs::metadata(&p).await.expect("must succeed");

        // However, calling either `readdir` or `stat` on a child should fail with an IO error.
        // It fails when trying to pull the first entry, because we don't implement opendir separately
        ReadDirStream::new(tokio::fs::read_dir(&p).await.unwrap())
            .next()
            .await
            .expect("must be some")
            .expect_err("must be err");

        // rust currently sets e.kind() to Uncategorized, which isn't very
        // helpful, so we don't look at the error more closely than that..
        tokio::fs::metadata(p.join(".keep"))
            .await
            .expect_err("must fail");
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

    let (blob_service, directory_service) = gen_svcs();
    let mut root_nodes = BTreeMap::default();

    populate_filenode_without_blob(&mut root_nodes).await;

    let mut fuse_daemon = do_mount(
        blob_service,
        directory_service,
        root_nodes,
        tmpdir.path(),
        false,
    )
    .expect("must succeed");

    let p = tmpdir.path().join(BLOB_A_NAME);

    {
        // `stat` on the blob should succeed, because it doesn't trigger a request to the blob service.
        tokio::fs::metadata(&p).await.expect("must succeed");

        // However, calling read on the blob should fail.
        // rust currently sets e.kind() to Uncategorized, which isn't very
        // helpful, so we don't look at the error more closely than that..
        tokio::fs::read(p).await.expect_err("must fail");
    }

    fuse_daemon.unmount().expect("unmount");
}
