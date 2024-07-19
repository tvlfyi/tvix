use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
};
use tokio::io::{self, AsyncWrite};

use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService};
use url::Url;

use crate::composition::{
    with_registry, Composition, DeserializeWithRegistry, ServiceBuilder, REG,
};
use crate::nar::{NarCalculationService, SimpleRenderer};
use crate::pathinfoservice::PathInfoService;

#[derive(serde::Deserialize, Default)]
pub struct CompositionConfigs {
    pub blobservices:
        HashMap<String, DeserializeWithRegistry<Box<dyn ServiceBuilder<Output = dyn BlobService>>>>,
    pub directoryservices: HashMap<
        String,
        DeserializeWithRegistry<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>>,
    >,
    pub pathinfoservices: HashMap<
        String,
        DeserializeWithRegistry<Box<dyn ServiceBuilder<Output = dyn PathInfoService>>>,
    >,
}

pub fn addrs_to_configs(
    blob_service_addr: impl AsRef<str>,
    directory_service_addr: impl AsRef<str>,
    path_info_service_addr: impl AsRef<str>,
) -> Result<CompositionConfigs, Box<dyn std::error::Error + Send + Sync>> {
    let mut configs: CompositionConfigs = Default::default();

    let blob_service_url = Url::parse(blob_service_addr.as_ref())?;
    let directory_service_url = Url::parse(directory_service_addr.as_ref())?;
    let path_info_service_url = Url::parse(path_info_service_addr.as_ref())?;

    configs.blobservices.insert(
        "default".into(),
        with_registry(&REG, || blob_service_url.try_into())?,
    );
    configs.directoryservices.insert(
        "default".into(),
        with_registry(&REG, || directory_service_url.try_into())?,
    );
    configs.pathinfoservices.insert(
        "default".into(),
        with_registry(&REG, || path_info_service_url.try_into())?,
    );

    Ok(configs)
}

/// Construct the store handles from their addrs.
pub async fn construct_services(
    blob_service_addr: impl AsRef<str>,
    directory_service_addr: impl AsRef<str>,
    path_info_service_addr: impl AsRef<str>,
) -> Result<
    (
        Arc<dyn BlobService>,
        Arc<dyn DirectoryService>,
        Arc<dyn PathInfoService>,
        Box<dyn NarCalculationService>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let configs = addrs_to_configs(
        blob_service_addr,
        directory_service_addr,
        path_info_service_addr,
    )?;
    construct_services_from_configs(configs).await
}

/// Construct the store handles from their addrs.
pub async fn construct_services_from_configs(
    configs: CompositionConfigs,
) -> Result<
    (
        Arc<dyn BlobService>,
        Arc<dyn DirectoryService>,
        Arc<dyn PathInfoService>,
        Box<dyn NarCalculationService>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let mut comp = Composition::default();

    comp.extend(configs.blobservices);
    comp.extend(configs.directoryservices);
    comp.extend(configs.pathinfoservices);

    let blob_service: Arc<dyn BlobService> = comp.build("default").await?;
    let directory_service: Arc<dyn DirectoryService> = comp.build("default").await?;
    let path_info_service: Arc<dyn PathInfoService> = comp.build("default").await?;

    // HACK: The grpc client also implements NarCalculationService, and we
    // really want to use it (otherwise we'd need to fetch everything again for hashing).
    // Until we revamped store composition and config, detect this special case here.
    let nar_calculation_service: Box<dyn NarCalculationService> = path_info_service
        .nar_calculation_service()
        .unwrap_or_else(|| {
            Box::new(SimpleRenderer::new(
                blob_service.clone(),
                directory_service.clone(),
            ))
        });

    Ok((
        blob_service,
        directory_service,
        path_info_service,
        nar_calculation_service,
    ))
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
