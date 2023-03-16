use count_write::CountWrite;
use sha2::{Digest, Sha256};

use crate::blobservice::BlobService;
use crate::chunkservice::ChunkService;
use crate::directoryservice::DirectoryService;
use crate::proto;

use super::renderer::NARRenderer;
use super::{NARCalculationService, RenderError};

/// A NAR calculation service which simply renders the whole NAR whenever
/// we ask for the calculation.
#[derive(Clone)]
pub struct NonCachingNARCalculationService<
    BS: BlobService,
    CS: ChunkService + Clone,
    DS: DirectoryService,
> {
    nar_renderer: NARRenderer<BS, CS, DS>,
}

impl<BS: BlobService, CS: ChunkService + Clone, DS: DirectoryService>
    NonCachingNARCalculationService<BS, CS, DS>
{
    pub fn new(blob_service: BS, chunk_service: CS, directory_service: DS) -> Self {
        Self {
            nar_renderer: NARRenderer::new(blob_service, chunk_service, directory_service),
        }
    }
}

impl<BS: BlobService, CS: ChunkService + Clone, DS: DirectoryService> NARCalculationService
    for NonCachingNARCalculationService<BS, CS, DS>
{
    fn calculate_nar(
        &self,
        root_node: &proto::node::Node,
    ) -> Result<proto::CalculateNarResponse, RenderError> {
        let h = Sha256::new();
        let mut cw = CountWrite::from(h);

        self.nar_renderer.write_nar(&mut cw, root_node)?;

        Ok(proto::CalculateNarResponse {
            nar_size: cw.count() as u32,
            nar_sha256: cw.into_inner().finalize().to_vec(),
        })
    }
}
