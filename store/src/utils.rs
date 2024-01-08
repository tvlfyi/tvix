use std::sync::Arc;
use std::{
    pin::Pin,
    task::{self, Poll},
};
use tokio::io::{self, AsyncWrite};

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

/// The inverse of [tokio_util::io::SyncIoBridge].
/// Don't use this with anything that actually does blocking I/O.
pub struct AsyncIoBridge<T>(pub T);

impl<W: std::io::Write + Unpin> AsyncWrite for AsyncIoBridge<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(self.get_mut().0.write(buf))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(self.get_mut().0.flush())
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }
}
