use crate::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    pathinfoservice::{MemoryPathInfoService, PathInfoService},
};

pub fn gen_blob_service() -> Box<dyn BlobService> {
    Box::new(MemoryBlobService::default())
}

pub fn gen_directory_service() -> Box<dyn DirectoryService> {
    Box::new(MemoryDirectoryService::default())
}

pub fn gen_pathinfo_service(
    blob_service: Box<dyn BlobService>,
    directory_service: Box<dyn DirectoryService>,
) -> impl PathInfoService {
    MemoryPathInfoService::new(blob_service, directory_service)
}
