use crate::pathinfoservice::{MemoryPathInfoService, PathInfoService};
use std::sync::Arc;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService};

pub use tvix_castore::utils::*;

pub fn gen_pathinfo_service<BS, DS>(
    blob_service: BS,
    directory_service: DS,
) -> Arc<dyn PathInfoService>
where
    BS: AsRef<dyn BlobService> + Send + Sync + 'static,
    DS: AsRef<dyn DirectoryService> + Send + Sync + 'static,
{
    Arc::new(MemoryPathInfoService::new(blob_service, directory_service))
}
