use crate::{
    blobservice::BlobService, directoryservice::DirectoryService, pathinfoservice::PathInfoService,
};
use std::sync::Arc;

pub struct FUSE<PS: PathInfoService> {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: PS,
}

impl<PS: PathInfoService> FUSE<PS> {
    pub fn new(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
        path_info_service: PS,
    ) -> Self {
        Self {
            blob_service,
            directory_service,
            path_info_service,
        }
    }
}

impl<PS: PathInfoService> fuser::Filesystem for FUSE<PS> {}
