use nix_compat::nar::reader::r#async as nar_reader;
use sha2::Digest;
use tokio::{
    io::{AsyncBufRead, AsyncRead},
    sync::mpsc,
    try_join,
};
use tvix_castore::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    import::{
        blobs::{self, ConcurrentBlobUploader},
        ingest_entries, IngestionEntry, IngestionError,
    },
    proto::{node::Node, NamedNode},
    PathBuf,
};

/// Ingests the contents from a [AsyncRead] providing NAR into the tvix store,
/// interacting with a [BlobService] and [DirectoryService].
/// Returns the castore root node, as well as the sha256 and size of the NAR
/// contents ingested.
pub async fn ingest_nar_and_hash<R, BS, DS>(
    blob_service: BS,
    directory_service: DS,
    r: &mut R,
) -> Result<(Node, [u8; 32], u64), IngestionError<Error>>
where
    R: AsyncRead + Unpin + Send,
    BS: BlobService + Clone + 'static,
    DS: DirectoryService,
{
    let mut nar_hash = sha2::Sha256::new();
    let mut nar_size = 0;

    // Assemble NarHash and NarSize as we read bytes.
    let r = tokio_util::io::InspectReader::new(r, |b| {
        nar_size += b.len() as u64;
        use std::io::Write;
        nar_hash.write_all(b).unwrap();
    });

    // HACK: InspectReader doesn't implement AsyncBufRead.
    // See if this can be propagated through and we can then require our input
    // reader to be buffered too.
    let mut r = tokio::io::BufReader::new(r);

    let root_node = ingest_nar(blob_service, directory_service, &mut r).await?;

    Ok((root_node, nar_hash.finalize().into(), nar_size))
}

/// Ingests the contents from a [AsyncRead] providing NAR into the tvix store,
/// interacting with a [BlobService] and [DirectoryService].
/// It returns the castore root node or an error.
pub async fn ingest_nar<R, BS, DS>(
    blob_service: BS,
    directory_service: DS,
    r: &mut R,
) -> Result<Node, IngestionError<Error>>
where
    R: AsyncBufRead + Unpin + Send,
    BS: BlobService + Clone + 'static,
    DS: DirectoryService,
{
    // open the NAR for reading.
    // The NAR reader emits nodes in DFS preorder.
    let root_node = nar_reader::open(r).await.map_err(Error::IO)?;

    let (tx, rx) = mpsc::channel(1);
    let rx = tokio_stream::wrappers::ReceiverStream::new(rx);

    let produce = async move {
        let mut blob_uploader = ConcurrentBlobUploader::new(blob_service);

        let res = produce_nar_inner(
            &mut blob_uploader,
            root_node,
            "root".parse().unwrap(), // HACK: the root node sent to ingest_entries may not be ROOT.
            tx.clone(),
        )
        .await;

        if let Err(err) = blob_uploader.join().await {
            tx.send(Err(err.into()))
                .await
                .map_err(|e| Error::IO(std::io::Error::new(std::io::ErrorKind::BrokenPipe, e)))?;
        }

        tx.send(res)
            .await
            .map_err(|e| Error::IO(std::io::Error::new(std::io::ErrorKind::BrokenPipe, e)))?;

        Ok(())
    };

    let consume = ingest_entries(directory_service, rx);

    let (_, node) = try_join!(produce, consume)?;

    // remove the fake "root" name again
    debug_assert_eq!(&node.get_name(), b"root");
    Ok(node.rename("".into()))
}

async fn produce_nar_inner<BS>(
    blob_uploader: &mut ConcurrentBlobUploader<BS>,
    node: nar_reader::Node<'_, '_>,
    path: PathBuf,
    tx: mpsc::Sender<Result<IngestionEntry, Error>>,
) -> Result<IngestionEntry, Error>
where
    BS: BlobService + Clone + 'static,
{
    Ok(match node {
        nar_reader::Node::Symlink { target } => IngestionEntry::Symlink { path, target },
        nar_reader::Node::File {
            executable,
            mut reader,
        } => {
            let size = reader.len();
            let digest = blob_uploader.upload(&path, size, &mut reader).await?;

            IngestionEntry::Regular {
                path,
                size,
                executable,
                digest,
            }
        }
        nar_reader::Node::Directory(mut dir_reader) => {
            while let Some(entry) = dir_reader.next().await? {
                let mut path = path.clone();

                // valid NAR names are valid castore names
                path.try_push(entry.name)
                    .expect("Tvix bug: failed to join name");

                let entry = Box::pin(produce_nar_inner(
                    blob_uploader,
                    entry.node,
                    path,
                    tx.clone(),
                ))
                .await?;

                tx.send(Ok(entry)).await.map_err(|e| {
                    Error::IO(std::io::Error::new(std::io::ErrorKind::BrokenPipe, e))
                })?;
            }

            IngestionEntry::Dir { path }
        }
    })
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    BlobUpload(#[from] blobs::Error),
}

#[cfg(test)]
mod test {
    use crate::nar::ingest_nar;
    use std::io::Cursor;
    use std::sync::Arc;

    use rstest::*;
    use tokio_stream::StreamExt;
    use tvix_castore::blobservice::BlobService;
    use tvix_castore::directoryservice::DirectoryService;
    use tvix_castore::fixtures::{
        DIRECTORY_COMPLICATED, DIRECTORY_WITH_KEEP, EMPTY_BLOB_DIGEST, HELLOWORLD_BLOB_CONTENTS,
        HELLOWORLD_BLOB_DIGEST,
    };
    use tvix_castore::proto as castorepb;

    use crate::tests::fixtures::{
        blob_service, directory_service, NAR_CONTENTS_COMPLICATED, NAR_CONTENTS_HELLOWORLD,
        NAR_CONTENTS_SYMLINK,
    };

    #[rstest]
    #[tokio::test]
    async fn single_symlink(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) {
        let root_node = ingest_nar(
            blob_service,
            directory_service,
            &mut Cursor::new(&NAR_CONTENTS_SYMLINK.clone()),
        )
        .await
        .expect("must parse");

        assert_eq!(
            castorepb::node::Node::Symlink(castorepb::SymlinkNode {
                name: "".into(), // name must be empty
                target: "/nix/store/somewhereelse".into(),
            }),
            root_node
        );
    }

    #[rstest]
    #[tokio::test]
    async fn single_file(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) {
        let root_node = ingest_nar(
            blob_service.clone(),
            directory_service,
            &mut Cursor::new(&NAR_CONTENTS_HELLOWORLD.clone()),
        )
        .await
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

    #[rstest]
    #[tokio::test]
    async fn complicated(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) {
        let root_node = ingest_nar(
            blob_service.clone(),
            directory_service.clone(),
            &mut Cursor::new(&NAR_CONTENTS_COMPLICATED.clone()),
        )
        .await
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
