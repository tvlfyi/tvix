use super::RenderError;
use async_recursion::async_recursion;
use count_write::CountWrite;
use nix_compat::nar::writer::r#async as nar_writer;
use sha2::{Digest, Sha256};
use std::{
    pin::Pin,
    task::{self, Poll},
};
use tokio::io::{self, AsyncWrite, BufReader};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tvix_castore::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    proto::{self as castorepb, NamedNode},
};

/// Invoke [write_nar], and return the size and sha256 digest of the produced
/// NAR output.
pub async fn calculate_size_and_sha256<BS, DS>(
    root_node: &castorepb::node::Node,
    blob_service: BS,
    directory_service: DS,
) -> Result<(u64, [u8; 32]), RenderError>
where
    BS: BlobService + Send,
    DS: DirectoryService + Send,
{
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
pub async fn write_nar<W, BS, DS>(
    w: W,
    proto_root_node: &castorepb::node::Node,
    blob_service: BS,
    directory_service: DS,
) -> Result<(), RenderError>
where
    W: AsyncWrite + Unpin + Send,
    BS: BlobService + Send,
    DS: DirectoryService + Send,
{
    // Initialize NAR writer
    let mut w = w.compat_write();
    let nar_root_node = nar_writer::open(&mut w)
        .await
        .map_err(RenderError::NARWriterError)?;

    walk_node(
        nar_root_node,
        proto_root_node,
        blob_service,
        directory_service,
    )
    .await?;

    Ok(())
}

/// Process an intermediate node in the structure.
/// This consumes the node.
#[async_recursion]
async fn walk_node<BS, DS>(
    nar_node: nar_writer::Node<'async_recursion, '_>,
    proto_node: &castorepb::node::Node,
    blob_service: BS,
    directory_service: DS,
) -> Result<(BS, DS), RenderError>
where
    BS: BlobService + Send,
    DS: DirectoryService + Send,
{
    match proto_node {
        castorepb::node::Node::Symlink(proto_symlink_node) => {
            nar_node
                .symlink(&proto_symlink_node.target)
                .await
                .map_err(RenderError::NARWriterError)?;
        }
        castorepb::node::Node::File(proto_file_node) => {
            let digest_len = proto_file_node.digest.len();
            let digest = proto_file_node.digest.clone().try_into().map_err(|_| {
                RenderError::StoreError(io::Error::new(
                    io::ErrorKind::Other,
                    format!("invalid digest len {} in file node", digest_len),
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
                    proto_file_node.size,
                    &mut blob_reader.compat(),
                )
                .await
                .map_err(RenderError::NARWriterError)?;
        }
        castorepb::node::Node::Directory(proto_directory_node) => {
            let digest_len = proto_directory_node.digest.len();
            let digest = proto_directory_node
                .digest
                .clone()
                .try_into()
                .map_err(|_| {
                    RenderError::StoreError(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid digest len {} in directory node", digest_len),
                    ))
                })?;

            // look it up with the directory service
            match directory_service
                .get(&digest)
                .await
                .map_err(|e| RenderError::StoreError(e.into()))?
            {
                // if it's None, that's an error!
                None => Err(RenderError::DirectoryNotFound(
                    digest,
                    proto_directory_node.name.clone(),
                ))?,
                Some(proto_directory) => {
                    // start a directory node
                    let mut nar_node_directory = nar_node
                        .directory()
                        .await
                        .map_err(RenderError::NARWriterError)?;

                    // We put blob_service, directory_service back here whenever we come up from
                    // the recursion.
                    let mut blob_service = blob_service;
                    let mut directory_service = directory_service;

                    // for each node in the directory, create a new entry with its name,
                    // and then recurse on that entry.
                    for proto_node in proto_directory.nodes() {
                        let child_node = nar_node_directory
                            .entry(proto_node.get_name())
                            .await
                            .map_err(RenderError::NARWriterError)?;

                        (blob_service, directory_service) =
                            walk_node(child_node, &proto_node, blob_service, directory_service)
                                .await?;
                    }

                    // close the directory
                    nar_node_directory
                        .close()
                        .await
                        .map_err(RenderError::NARWriterError)?;

                    return Ok((blob_service, directory_service));
                }
            }
        }
    }

    Ok((blob_service, directory_service))
}
