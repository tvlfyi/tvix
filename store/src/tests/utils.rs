use std::sync::Arc;

use crate::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    pathinfoservice::{MemoryPathInfoService, PathInfoService},
};

pub fn gen_blob_service() -> Arc<dyn BlobService> {
    Arc::new(MemoryBlobService::default())
}

pub fn gen_directory_service() -> Arc<dyn DirectoryService> {
    Arc::new(MemoryDirectoryService::default())
}

pub fn gen_pathinfo_service(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> impl PathInfoService {
    MemoryPathInfoService::new(blob_service, directory_service)
}
