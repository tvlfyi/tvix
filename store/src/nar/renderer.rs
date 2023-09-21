use super::RenderError;
use count_write::CountWrite;
use nix_compat::nar;
use sha2::{Digest, Sha256};
use std::{io, sync::Arc};
use tokio::{io::BufReader, task::spawn_blocking};
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
    let h = Sha256::new();
    let cw = CountWrite::from(h);

    let cw = write_nar(cw, root_node, blob_service, directory_service).await?;

    Ok((cw.count(), cw.into_inner().finalize().into()))
}

/// Accepts a [castorepb::node::Node] pointing to the root of a (store) path,
/// and uses the passed blob_service and directory_service to perform the
/// necessary lookups as it traverses the structure.
/// The contents in NAR serialization are writen to the passed [std::io::Write].
///
/// The writer is passed back in the return value. This is done because async Rust
/// lacks scoped blocking tasks, so we need to transfer ownership of the writer
/// internally.
///
/// # Panics
/// This will panic if called outside the context of a Tokio runtime.
pub async fn write_nar<W: std::io::Write + Send + 'static>(
    mut w: W,
    proto_root_node: &castorepb::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<W, RenderError> {
    let tokio_handle = tokio::runtime::Handle::current();
    let proto_root_node = proto_root_node.clone();

    spawn_blocking(move || {
        // Initialize NAR writer
        let nar_root_node = nar::writer::open(&mut w).map_err(RenderError::NARWriterError)?;

        walk_node(
            tokio_handle,
            nar_root_node,
            &proto_root_node,
            blob_service,
            directory_service,
        )?;

        Ok(w)
    })
    .await
    .unwrap()
}

/// Process an intermediate node in the structure.
/// This consumes the node.
fn walk_node(
    tokio_handle: tokio::runtime::Handle,
    nar_node: nar::writer::Node,
    proto_node: &castorepb::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<(), RenderError> {
    match proto_node {
        castorepb::node::Node::Symlink(proto_symlink_node) => {
            nar_node
                .symlink(&proto_symlink_node.target)
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

            let blob_reader = match tokio_handle
                .block_on(async { blob_service.open_read(&digest).await })
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
                    &mut tokio_util::io::SyncIoBridge::new(blob_reader),
                )
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
            match tokio_handle
                .block_on(async { directory_service.get(&digest).await })
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
                    let mut nar_node_directory =
                        nar_node.directory().map_err(RenderError::NARWriterError)?;

                    // for each node in the directory, create a new entry with its name,
                    // and then invoke walk_node on that entry.
                    for proto_node in proto_directory.nodes() {
                        let child_node = nar_node_directory
                            .entry(proto_node.get_name())
                            .map_err(RenderError::NARWriterError)?;
                        walk_node(
                            tokio_handle.clone(),
                            child_node,
                            &proto_node,
                            blob_service.clone(),
                            directory_service.clone(),
                        )?;
                    }

                    // close the directory
                    nar_node_directory
                        .close()
                        .map_err(RenderError::NARWriterError)?;
                }
            }
        }
    }
    Ok(())
}
