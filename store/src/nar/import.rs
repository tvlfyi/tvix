use std::{
    io::{self, Read},
    sync::Arc,
};

use bytes::Bytes;
use nix_compat::nar;
use tokio_util::io::SyncIoBridge;
use tracing::warn;
use tvix_castore::{
    blobservice::BlobService,
    directoryservice::{DirectoryPutter, DirectoryService},
    proto::{self as castorepb},
    B3Digest,
};

/// Accepts a reader providing a NAR.
/// Will traverse it, uploading blobs to the given [BlobService], and
/// directories to the given [DirectoryService].
/// On success, the root node is returned.
/// This function is not async (because the NAR reader is not)
/// and calls [tokio::task::block_in_place] when interacting with backing
/// services, so make sure to only call this with spawn_blocking.
pub fn read_nar<R: Read + Send>(
    r: &mut R,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> io::Result<castorepb::node::Node> {
    let handle = tokio::runtime::Handle::current();

    let directory_putter = directory_service.put_multiple_start();

    let node = nix_compat::nar::reader::open(r)?;
    let (root_node, mut directory_putter) = process_node(
        handle.clone(),
        "".into(), // this is the root node, it has an empty name
        node,
        blob_service,
        directory_putter,
    )?;

    // In case the root node points to a directory, we need to close
    // [directory_putter], and ensure the digest we got back from there matches
    // what the root node is pointing to.
    if let castorepb::node::Node::Directory(ref directory_node) = root_node {
        // Close directory_putter to make sure all directories have been inserted.
        let directory_putter_digest =
            handle.block_on(handle.spawn(async move { directory_putter.close().await }))??;
        let root_directory_node_digest: B3Digest =
            directory_node.digest.clone().try_into().unwrap();

        if directory_putter_digest != root_directory_node_digest {
            warn!(
                root_directory_node_digest = %root_directory_node_digest,
                directory_putter_digest =%directory_putter_digest,
                "directory digest mismatch",
            );
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "directory digest mismatch",
            ));
        }
    }
    // In case it's not a Directory, [directory_putter] doesn't need to be
    // closed (as we didn't end up uploading anything).
    // It can just be dropped, as documented in its trait.

    Ok(root_node)
}

/// This is called on a [nar::reader::Node] and returns a [castorepb::node::Node].
/// It does so by handling all three kinds, and recursing for directories.
///
/// [DirectoryPutter] is passed around, so a single instance of it can be used,
/// which is sufficient, as this reads through the whole NAR linerarly.
fn process_node(
    handle: tokio::runtime::Handle,
    name: bytes::Bytes,
    node: nar::reader::Node,
    blob_service: Arc<dyn BlobService>,
    directory_putter: Box<dyn DirectoryPutter>,
) -> io::Result<(castorepb::node::Node, Box<dyn DirectoryPutter>)> {
    Ok(match node {
        nar::reader::Node::Symlink { target } => (
            castorepb::node::Node::Symlink(castorepb::SymlinkNode {
                name,
                target: target.into(),
            }),
            directory_putter,
        ),
        nar::reader::Node::File { executable, reader } => (
            castorepb::node::Node::File(process_file_reader(
                handle,
                name,
                reader,
                executable,
                blob_service,
            )?),
            directory_putter,
        ),
        nar::reader::Node::Directory(dir_reader) => {
            let (directory_node, directory_putter) = process_dir_reader(
                handle,
                name,
                dir_reader,
                blob_service.clone(),
                directory_putter,
            )?;

            (
                castorepb::node::Node::Directory(directory_node),
                directory_putter,
            )
        }
    })
}

/// Given a name and [nar::reader::FileReader], this ingests the file into the
/// passed [BlobService] and returns a [castorepb::FileNode].
fn process_file_reader(
    handle: tokio::runtime::Handle,
    name: Bytes,
    mut file_reader: nar::reader::FileReader,
    executable: bool,
    blob_service: Arc<dyn BlobService>,
) -> io::Result<castorepb::FileNode> {
    // store the length. If we read any other length, reading will fail.
    let expected_len = file_reader.len();

    // prepare writing a new blob.
    let blob_writer = handle.block_on(handle.spawn({
        let blob_service = blob_service.clone();
        async move { blob_service.open_write().await }
    }))?;

    // write the blob.
    let mut blob_writer = {
        let mut dest = SyncIoBridge::new(blob_writer);
        io::copy(&mut file_reader, &mut dest)?;

        dest.shutdown()?;

        // return back the blob_reader
        dest.into_inner()
    };

    // close the blob_writer, retrieve the digest.
    let blob_digest = handle.block_on(handle.spawn(async move { blob_writer.close().await }))??;

    Ok(castorepb::FileNode {
        name,
        digest: blob_digest.into(),
        size: expected_len,
        executable,
    })
}

/// Given a name and [nar::reader::DirReader], this returns a [castorepb::DirectoryNode].
/// It uses [process_node] to iterate over all children.
///
/// [DirectoryPutter] is passed around, so a single instance of it can be used,
/// which is sufficient, as this reads through the whole NAR linerarly.
fn process_dir_reader(
    handle: tokio::runtime::Handle,
    name: Bytes,
    mut dir_reader: nar::reader::DirReader,
    blob_service: Arc<dyn BlobService>,
    directory_putter: Box<dyn DirectoryPutter>,
) -> io::Result<(castorepb::DirectoryNode, Box<dyn DirectoryPutter>)> {
    let mut directory = castorepb::Directory::default();

    let mut directory_putter = directory_putter;
    while let Some(entry) = dir_reader.next()? {
        let (node, directory_putter_back) = process_node(
            handle.clone(),
            entry.name.into(),
            entry.node,
            blob_service.clone(),
            directory_putter,
        )?;

        directory_putter = directory_putter_back;

        match node {
            castorepb::node::Node::Directory(node) => directory.directories.push(node),
            castorepb::node::Node::File(node) => directory.files.push(node),
            castorepb::node::Node::Symlink(node) => directory.symlinks.push(node),
        }
    }

    // calculate digest and size.
    let directory_digest = directory.digest();
    let directory_size = directory.size();

    // upload the directory. This is a bit more verbose, as we want to get back
    // directory_putter for later reuse.
    let directory_putter = handle.block_on(handle.spawn(async move {
        directory_putter.put(directory).await?;
        Ok::<_, io::Error>(directory_putter)
    }))??;

    Ok((
        castorepb::DirectoryNode {
            name,
            digest: directory_digest.into(),
            size: directory_size,
        },
        directory_putter,
    ))
}

#[cfg(test)]
mod test {
    use crate::nar::read_nar;
    use std::io::Cursor;

    use tokio_stream::StreamExt;
    use tvix_castore::fixtures::{
        DIRECTORY_COMPLICATED, DIRECTORY_WITH_KEEP, EMPTY_BLOB_DIGEST, HELLOWORLD_BLOB_CONTENTS,
        HELLOWORLD_BLOB_DIGEST,
    };
    use tvix_castore::proto as castorepb;
    use tvix_castore::utils::{gen_blob_service, gen_directory_service};

    use crate::tests::fixtures::{
        NAR_CONTENTS_COMPLICATED, NAR_CONTENTS_HELLOWORLD, NAR_CONTENTS_SYMLINK,
    };

    #[tokio::test]
    async fn single_symlink() {
        let handle = tokio::runtime::Handle::current();

        let root_node = handle
            .spawn_blocking(|| {
                read_nar(
                    &mut Cursor::new(&NAR_CONTENTS_SYMLINK.clone()),
                    gen_blob_service(),
                    gen_directory_service(),
                )
            })
            .await
            .unwrap()
            .expect("must parse");

        assert_eq!(
            castorepb::node::Node::Symlink(castorepb::SymlinkNode {
                name: "".into(), // name must be empty
                target: "/nix/store/somewhereelse".into(),
            }),
            root_node
        );
    }

    #[tokio::test]
    async fn single_file() {
        let blob_service = gen_blob_service();

        let handle = tokio::runtime::Handle::current();

        let root_node = handle
            .spawn_blocking({
                let blob_service = blob_service.clone();
                || {
                    read_nar(
                        &mut Cursor::new(&NAR_CONTENTS_HELLOWORLD.clone()),
                        blob_service,
                        gen_directory_service(),
                    )
                }
            })
            .await
            .unwrap()
            .expect("must parse");

        assert_eq!(
            castorepb::node::Node::File(castorepb::FileNode {
                name: "".into(), // name must be empty
                digest: HELLOWORLD_BLOB_DIGEST.clone().into(),
                size: HELLOWORLD_BLOB_CONTENTS.len() as u64,
                executable: false,
            }),
            root_node
        );

        // blobservice must contain the blob
        assert!(blob_service.has(&HELLOWORLD_BLOB_DIGEST).await.unwrap());
    }

    #[tokio::test]
    async fn complicated() {
        let blob_service = gen_blob_service();
        let directory_service = gen_directory_service();

        let handle = tokio::runtime::Handle::current();

        let root_node = handle
            .spawn_blocking({
                let blob_service = blob_service.clone();
                let directory_service = directory_service.clone();
                || {
                    read_nar(
                        &mut Cursor::new(&NAR_CONTENTS_COMPLICATED.clone()),
                        blob_service,
                        directory_service,
                    )
                }
            })
            .await
            .unwrap()
            .expect("must parse");

        assert_eq!(
            castorepb::node::Node::Directory(castorepb::DirectoryNode {
                name: "".into(), // name must be empty
                digest: DIRECTORY_COMPLICATED.digest().into(),
                size: DIRECTORY_COMPLICATED.size(),
            }),
            root_node,
        );

        // blobservice must contain the blob
        assert!(blob_service.has(&EMPTY_BLOB_DIGEST).await.unwrap());

        // directoryservice must contain the directories, at least with get_recursive.
        let resp: Result<Vec<castorepb::Directory>, _> = directory_service
            .get_recursive(&DIRECTORY_COMPLICATED.digest())
            .collect()
            .await;

        let directories = resp.unwrap();

        assert_eq!(2, directories.len());
        assert_eq!(DIRECTORY_COMPLICATED.clone(), directories[0]);
        assert_eq!(DIRECTORY_WITH_KEEP.clone(), directories[1]);
    }
}
