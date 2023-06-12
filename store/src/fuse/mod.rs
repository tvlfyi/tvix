use crate::{
    blobservice::BlobService, directoryservice::DirectoryService, pathinfoservice::PathInfoService,
};
use std::sync::Arc;

pub struct FUSE {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,
}

impl FUSE {
    pub fn new(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
        path_info_service: Arc<dyn PathInfoService>,
    ) -> Self {
        Self {
            blob_service,
            directory_service,
            path_info_service,
        }
    }
}

impl fuser::Filesystem for FUSE {}
