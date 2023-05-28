use crate::{
    blobservice::BlobService, directoryservice::DirectoryService, pathinfoservice::PathInfoService,
};

pub struct FUSE<BS: BlobService, DS: DirectoryService, PS: PathInfoService> {
    blob_service: BS,
    directory_service: DS,
    path_info_service: PS,
}

impl<BS: BlobService, DS: DirectoryService, PS: PathInfoService> FUSE<BS, DS, PS> {
    pub fn new(path_info_service: PS, directory_service: DS, blob_service: BS) -> Self {
        Self {
            blob_service,
            path_info_service,
            directory_service,
        }
    }
}

impl<BS: BlobService, DS: DirectoryService, PS: PathInfoService> fuser::Filesystem
    for FUSE<BS, DS, PS>
{
}
