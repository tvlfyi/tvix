use super::RenderError;
use async_recursion::async_recursion;
use count_write::CountWrite;
use nix_compat::nar::writer::r#async as nar_writer;
use sha2::{Digest, Sha256};
use std::{
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
};
use tokio::io::{self, AsyncWrite, BufReader};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::warn;
use tvix_castore::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    proto::{self as castorepb, NamedNode},
    Error,
};

/// Invoke [write_nar], and return the size and sha256 digest of the produced
/// NAR output.
pub async fn calculate_size_and_sha256(
    root_node: &castorepb::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<(u64, [u8; 32]), RenderError> {
    let mut h = Sha256::new();
    let mut cw = CountWrite::from(&mut h);

    write_nar(
        // The hasher doesn't speak async. It doesn't
        // actually do any I/O, so it's fine to wrap.
        AsyncIoBridge(&mut cw),
        root_node,
        blob_service,
        directory_service,
    )
    .await?;

    Ok((cw.count(), h.finalize().into()))
}

/// The inverse of [tokio_util::io::SyncIoBridge].
/// Don't use this with anything that actually does blocking I/O.
struct AsyncIoBridge<T>(T);

impl<W: std::io::Write + Unpin> AsyncWrite for AsyncIoBridge<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(self.get_mut().0.write(buf))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.get_mut().0.flush())
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}

/// Accepts a [castorepb::node::Node] pointing to the root of a (store) path,
/// and uses the passed blob_service and directory_service to perform the
/// necessary lookups as it traverses the structure.
/// The contents in NAR serialization are writen to the passed [AsyncWrite].
pub async fn write_nar<W: AsyncWrite + Unpin + Send>(
    w: W,
    proto_root_node: &castorepb::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<(), RenderError> {
    // Initialize NAR writer
    let mut w = w.compat_write();
    let nar_root_node = nar_writer::open(&mut w)
        .await
        .map_err(RenderError::NARWriterError)?;

    walk_node(
        nar_root_node,
        &proto_root_node,
        blob_service,
        directory_service,
    )
    .await?;

    Ok(())
}

/// Process an intermediate node in the structure.
/// This consumes the node.
#[async_recursion]
async fn walk_node(
    nar_node: nar_writer::Node<'async_recursion, '_>,
    proto_node: &castorepb::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<(), RenderError> {
    match proto_node {
        castorepb::node::Node::Symlink(proto_symlink_node) => {
            nar_node
                .symlink(&proto_symlink_node.target)
                .await
                .map_err(RenderError::NARWriterError)?;
        }
        castorepb::node::Node::File(proto_file_node) => {
            let digest = proto_file_node.digest.clone().try_into().map_err(|_e| {
                warn!(
                    file_node = ?proto_file_node,
                    "invalid digest length in file node",
                );

                RenderError::StoreError(Error::StorageError(
                    "invalid digest len in file node".to_string(),
                ))
            })?;

            let blob_reader = match blob_service
                .open_read(&digest)
                .await
                .map_err(RenderError::StoreError)?
            {
                Some(blob_reader) => Ok(BufReader::new(blob_reader)),
                None => Err(RenderError::NARWriterError(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("blob with digest {} not found", &digest),
                ))),
            }?;

            nar_node
                .file(
                    proto_file_node.executable,
                    proto_file_node.size.into(),
                    &mut blob_reader.compat(),
                )
                .await
                .map_err(RenderError::NARWriterError)?;
        }
        castorepb::node::Node::Directory(proto_directory_node) => {
            let digest = proto_directory_node
                .digest
                .clone()
                .try_into()
                .map_err(|_e| {
                    RenderError::StoreError(Error::StorageError(
                        "invalid digest len in directory node".to_string(),
                    ))
                })?;

            // look it up with the directory service
            match directory_service
                .get(&digest)
                .await
                .map_err(RenderError::StoreError)?
            {
                // if it's None, that's an error!
                None => {
                    return Err(RenderError::DirectoryNotFound(
                        digest,
                        proto_directory_node.name.clone(),
                    ))
                }
                Some(proto_directory) => {
                    // start a directory node
                    let mut nar_node_directory = nar_node
                        .directory()
                        .await
                        .map_err(RenderError::NARWriterError)?;

                    // for each node in the directory, create a new entry with its name,
                    // and then invoke walk_node on that entry.
                    for proto_node in proto_directory.nodes() {
                        let child_node = nar_node_directory
                            .entry(proto_node.get_name())
                            .await
                            .map_err(RenderError::NARWriterError)?;
                        walk_node(
                            child_node,
                            &proto_node,
                            blob_service.clone(),
                            directory_service.clone(),
                        )
                        .await?;
                    }

                    // close the directory
                    nar_node_directory
                        .close()
                        .await
                        .map_err(RenderError::NARWriterError)?;
                }
            }
        }
    }
    Ok(())
}
