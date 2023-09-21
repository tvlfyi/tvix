use crate::pathinfoservice::{MemoryPathInfoService, PathInfoService};
use std::sync::Arc;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService};

pub use tvix_castore::utils::*;

pub fn gen_pathinfo_service(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) -> Arc<dyn PathInfoService> {
    Arc::new(MemoryPathInfoService::new(blob_service, directory_service))
}
