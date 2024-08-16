use crate::utils::AsyncIoBridge;

use super::{NarCalculationService, RenderError};
use count_write::CountWrite;
use nix_compat::nar::writer::r#async as nar_writer;
use sha2::{Digest, Sha256};
use tokio::io::{self, AsyncWrite, BufReader};
use tonic::async_trait;
use tracing::{instrument, Span};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService, Node};

pub struct SimpleRenderer<BS, DS> {
    blob_service: BS,
    directory_service: DS,
}

impl<BS, DS> SimpleRenderer<BS, DS> {
    pub fn new(blob_service: BS, directory_service: DS) -> Self {
        Self {
            blob_service,
            directory_service,
        }
    }
}

#[async_trait]
impl<BS, DS> NarCalculationService for SimpleRenderer<BS, DS>
where
    BS: BlobService + Clone,
    DS: DirectoryService + Clone,
{
    async fn calculate_nar(
        &self,
        root_node: &Node,
    ) -> Result<(u64, [u8; 32]), tvix_castore::Error> {
        calculate_size_and_sha256(
            root_node,
            self.blob_service.clone(),
            self.directory_service.clone(),
        )
        .await
        .map_err(|e| tvix_castore::Error::StorageError(format!("failed rendering nar: {}", e)))
    }
}

/// Invoke [write_nar], and return the size and sha256 digest of the produced
/// NAR output.
#[instrument(skip_all, fields(indicatif.pb_show=1))]
pub async fn calculate_size_and_sha256<BS, DS>(
    root_node: &Node,
    blob_service: BS,
    directory_service: DS,
) -> Result<(u64, [u8; 32]), RenderError>
where
    BS: BlobService + Send,
    DS: DirectoryService + Send,
{
    let mut h = Sha256::new();
    let mut cw = CountWrite::from(&mut h);

    let span = Span::current();
    span.pb_set_message("Calculating NAR");
    span.pb_start();

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

/// Accepts a [Node] pointing to the root of a (store) path,
/// and uses the passed blob_service and directory_service to perform the
/// necessary lookups as it traverses the structure.
/// The contents in NAR serialization are writen to the passed [AsyncWrite].
pub async fn write_nar<W, BS, DS>(
    mut w: W,
    root_node: &Node,
    blob_service: BS,
    directory_service: DS,
) -> Result<(), RenderError>
where
    W: AsyncWrite + Unpin + Send,
    BS: BlobService + Send,
    DS: DirectoryService + Send,
{
    // Initialize NAR writer
    let nar_root_node = nar_writer::open(&mut w)
        .await
        .map_err(RenderError::NARWriterError)?;

    walk_node(
        nar_root_node,
        root_node,
        b"",
        blob_service,
        directory_service,
    )
    .await?;

    Ok(())
}

/// Process an intermediate node in the structure.
/// This consumes the node.
async fn walk_node<BS, DS>(
    nar_node: nar_writer::Node<'_, '_>,
    castore_node: &Node,
    name: &[u8],
    blob_service: BS,
    directory_service: DS,
) -> Result<(BS, DS), RenderError>
where
    BS: BlobService + Send,
    DS: DirectoryService + Send,
{
    match castore_node {
        Node::Symlink { target, .. } => {
            nar_node
                .symlink(target.as_ref())
                .await
                .map_err(RenderError::NARWriterError)?;
        }
        Node::File {
            digest,
            size,
            executable,
        } => {
            let mut blob_reader = match blob_service
                .open_read(digest)
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
                .file(*executable, *size, &mut blob_reader)
                .await
                .map_err(RenderError::NARWriterError)?;
        }
        Node::Directory { digest, .. } => {
            // look it up with the directory service
            match directory_service
                .get(digest)
                .await
                .map_err(|e| RenderError::StoreError(e.into()))?
            {
                // if it's None, that's an error!
                None => Err(RenderError::DirectoryNotFound(
                    digest.clone(),
                    bytes::Bytes::copy_from_slice(name),
                ))?,
                Some(directory) => {
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
                    for (name, node) in directory.nodes() {
                        let child_node = nar_node_directory
                            .entry(name.as_ref())
                            .await
                            .map_err(RenderError::NARWriterError)?;

                        (blob_service, directory_service) = Box::pin(walk_node(
                            child_node,
                            node,
                            name.as_ref(),
                            blob_service,
                            directory_service,
                        ))
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
