use super::RenderError;
use crate::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    proto::{self, NamedNode},
};
use count_write::CountWrite;
use nix_compat::nar;
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufReader},
    sync::Arc,
};
use tracing::warn;

/// Invoke [write_nar], and return the size and sha256 digest of the produced
/// NAR output.
pub fn calculate_size_and_sha256(
    root_node: &proto::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<(u64, [u8; 32]), RenderError> {
    let h = Sha256::new();
    let mut cw = CountWrite::from(h);

    write_nar(&mut cw, root_node, blob_service, directory_service)?;

    Ok((cw.count(), cw.into_inner().finalize().into()))
}

/// Accepts a [proto::node::Node] pointing to the root of a (store) path,
/// and uses the passed blob_service and directory_service to
/// perform the necessary lookups as it traverses the structure.
/// The contents in NAR serialization are writen to the passed [std::io::Write].
pub fn write_nar<W: std::io::Write>(
    w: &mut W,
    proto_root_node: &proto::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<(), RenderError> {
    // Initialize NAR writer
    let nar_root_node = nar::writer::open(w).map_err(RenderError::NARWriterError)?;

    walk_node(
        nar_root_node,
        proto_root_node,
        blob_service,
        directory_service,
    )
}

/// Process an intermediate node in the structure.
/// This consumes the node.
fn walk_node(
    nar_node: nar::writer::Node,
    proto_node: &proto::node::Node,
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Result<(), RenderError> {
    match proto_node {
        proto::node::Node::Symlink(proto_symlink_node) => {
            nar_node
                .symlink(&proto_symlink_node.target)
                .map_err(RenderError::NARWriterError)?;
        }
        proto::node::Node::File(proto_file_node) => {
            let digest = proto_file_node.digest.clone().try_into().map_err(|_e| {
                warn!(
                    file_node = ?proto_file_node,
                    "invalid digest length in file node",
                );

                RenderError::StoreError(crate::Error::StorageError(
                    "invalid digest len in file node".to_string(),
                ))
            })?;

            let mut blob_reader = match blob_service
                .open_read(&digest)
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
                    &mut blob_reader,
                )
                .map_err(RenderError::NARWriterError)?;
        }
        proto::node::Node::Directory(proto_directory_node) => {
            let digest = proto_directory_node
                .digest
                .clone()
                .try_into()
                .map_err(|_e| {
                    RenderError::StoreError(crate::Error::StorageError(
                        "invalid digest len in directory node".to_string(),
                    ))
                })?;

            // look it up with the directory service
            let resp = directory_service
                .get(&digest)
                .map_err(RenderError::StoreError)?;

            match resp {
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
