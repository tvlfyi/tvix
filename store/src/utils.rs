use std::sync::Arc;

use tvix_castore::{
    blobservice::{self, BlobService},
    directoryservice::{self, DirectoryService},
};

use crate::pathinfoservice::{self, PathInfoService};

/// Construct the three store handles from their addrs.
pub async fn construct_services(
    blob_service_addr: impl AsRef<str>,
    directory_service_addr: impl AsRef<str>,
    path_info_service_addr: impl AsRef<str>,
) -> std::io::Result<(
    Arc<dyn BlobService>,
    Arc<dyn DirectoryService>,
    Box<dyn PathInfoService>,
)> {
    let blob_service: Arc<dyn BlobService> = blobservice::from_addr(blob_service_addr.as_ref())
        .await?
        .into();
    let directory_service: Arc<dyn DirectoryService> =
        directoryservice::from_addr(directory_service_addr.as_ref())
            .await?
            .into();
    let path_info_service = pathinfoservice::from_addr(
        path_info_service_addr.as_ref(),
        blob_service.clone(),
        directory_service.clone(),
    )
    .await?;

    Ok((blob_service, directory_service, path_info_service))
}
