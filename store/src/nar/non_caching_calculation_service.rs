use count_write::CountWrite;
use sha2::{Digest, Sha256};

use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryService;
use crate::proto;

use super::renderer::NARRenderer;
use super::{NARCalculationService, RenderError};

/// A NAR calculation service which simply renders the whole NAR whenever
/// we ask for the calculation.
pub struct NonCachingNARCalculationService<DS: DirectoryService> {
    nar_renderer: NARRenderer<DS>,
}

impl<DS: DirectoryService> NonCachingNARCalculationService<DS> {
    pub fn new(blob_service: Box<dyn BlobService>, directory_service: DS) -> Self {
        Self {
            nar_renderer: NARRenderer::new(blob_service, directory_service),
        }
    }
}

impl<DS: DirectoryService> NARCalculationService for NonCachingNARCalculationService<DS> {
    fn calculate_nar(&self, root_node: &proto::node::Node) -> Result<(u64, [u8; 32]), RenderError> {
        let h = Sha256::new();
        let mut cw = CountWrite::from(h);

        self.nar_renderer.write_nar(&mut cw, root_node)?;

        Ok((cw.count(), cw.into_inner().finalize().into()))
    }
}
