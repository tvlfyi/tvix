use super::RenderError;
use crate::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    proto::{self, NamedNode},
    B3Digest,
};
use nix_compat::nar;
use std::io::{self, BufReader};
use tracing::warn;

/// A NAR renderer, using a blob_service, chunk_service and directory_service
/// to render a NAR to a writer.
pub struct NARRenderer<DS: DirectoryService> {
    blob_service: Box<dyn BlobService>,
    directory_service: DS,
}

impl<DS: DirectoryService> NARRenderer<DS> {
    pub fn new(blob_service: Box<dyn BlobService>, directory_service: DS) -> Self {
        Self {
            blob_service,
            directory_service,
        }
    }

    /// Consumes a [proto::node::Node] pointing to the root of a (store) path,
    /// and writes the contents in NAR serialization to the passed
    /// [std::io::Write].
    ///
    /// It uses the different clients in the struct to perform the necessary
    /// lookups as it traverses the structure.
    pub fn write_nar<W: std::io::Write>(
        &self,
        w: &mut W,
        proto_root_node: &proto::node::Node,
    ) -> Result<(), RenderError> {
        // Initialize NAR writer
        let nar_root_node = nar::writer::open(w).map_err(RenderError::NARWriterError)?;

        self.walk_node(nar_root_node, proto_root_node)
    }

    /// Process an intermediate node in the structure.
    /// This consumes the node.
    fn walk_node(
        &self,
        nar_node: nar::writer::Node,
        proto_node: &proto::node::Node,
    ) -> Result<(), RenderError> {
        match proto_node {
            proto::node::Node::Symlink(proto_symlink_node) => {
                nar_node
                    .symlink(&proto_symlink_node.target)
                    .map_err(RenderError::NARWriterError)?;
            }
            proto::node::Node::File(proto_file_node) => {
                let digest = B3Digest::from_vec(proto_file_node.digest.clone()).map_err(|_e| {
                    warn!(
                        file_node = ?proto_file_node,
                        "invalid digest length in file node",
                    );

                    RenderError::StoreError(crate::Error::StorageError(
                        "invalid digest len in file node".to_string(),
                    ))
                })?;

                let mut blob_reader = match self
                    .blob_service
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
                let digest =
                    B3Digest::from_vec(proto_directory_node.digest.to_vec()).map_err(|_e| {
                        RenderError::StoreError(crate::Error::StorageError(
                            "invalid digest len in directory node".to_string(),
                        ))
                    })?;

                // look it up with the directory service
                let resp = self
                    .directory_service
                    .get(&digest)
                    .map_err(RenderError::StoreError)?;

                match resp {
                    // if it's None, that's an error!
                    None => {
                        return Err(RenderError::DirectoryNotFound(
                            digest,
                            proto_directory_node.name.to_owned(),
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
                            self.walk_node(child_node, &proto_node)?;
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
}
