use crate::{
    blobservice::BlobService,
    chunkservice::ChunkService,
    directoryservice::DirectoryService,
    proto::{self, NamedNode},
    BlobReader,
};
use nix_compat::nar;

use super::RenderError;

/// A NAR renderer, using a blob_service, chunk_service and directory_service
/// to render a NAR to a writer.
#[derive(Clone)]
pub struct NARRenderer<BS: BlobService, CS: ChunkService + Clone, DS: DirectoryService> {
    blob_service: BS,
    chunk_service: CS,
    directory_service: DS,
}

impl<BS: BlobService, CS: ChunkService + Clone, DS: DirectoryService> NARRenderer<BS, CS, DS> {
    pub fn new(blob_service: BS, chunk_service: CS, directory_service: DS) -> Self {
        Self {
            blob_service,
            chunk_service,
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
        proto_root_node: proto::node::Node,
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
        proto_node: proto::node::Node,
    ) -> Result<(), RenderError> {
        match proto_node {
            proto::node::Node::Symlink(proto_symlink_node) => {
                nar_node
                    .symlink(&proto_symlink_node.target)
                    .map_err(RenderError::NARWriterError)?;
            }
            proto::node::Node::File(proto_file_node) => {
                // get the digest we're referring to
                let digest = proto_file_node.digest;
                // query blob_service for blob_meta
                let resp = self
                    .blob_service
                    .stat(&proto::StatBlobRequest {
                        digest: digest.to_vec(),
                        include_chunks: true,
                        ..Default::default()
                    })
                    .map_err(RenderError::StoreError)?;

                match resp {
                    // if it's None, that's an error!
                    None => {
                        return Err(RenderError::BlobNotFound(digest, proto_file_node.name));
                    }
                    Some(blob_meta) => {
                        // make sure the blob_meta size matches what we expect from proto_file_node
                        let blob_meta_size = blob_meta.chunks.iter().fold(0, |acc, e| acc + e.size);
                        if blob_meta_size != proto_file_node.size {
                            return Err(RenderError::UnexpectedBlobMeta(
                                digest,
                                proto_file_node.name,
                                proto_file_node.size,
                                blob_meta_size,
                            ));
                        }

                        let mut blob_reader = std::io::BufReader::new(BlobReader::open(
                            &self.chunk_service,
                            blob_meta,
                        ));
                        nar_node
                            .file(
                                proto_file_node.executable,
                                proto_file_node.size.into(),
                                &mut blob_reader,
                            )
                            .map_err(RenderError::NARWriterError)?;
                    }
                }
            }
            proto::node::Node::Directory(proto_directory_node) => {
                // get the digest we're referring to
                let digest = proto_directory_node.digest;
                // look it up with the directory service
                let resp = self
                    .directory_service
                    .get(&proto::get_directory_request::ByWhat::Digest(
                        digest.to_vec(),
                    ))
                    .map_err(RenderError::StoreError)?;

                match resp {
                    // if it's None, that's an error!
                    None => {
                        return Err(RenderError::DirectoryNotFound(
                            digest,
                            proto_directory_node.name,
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
                            self.walk_node(child_node, proto_node)?;
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
