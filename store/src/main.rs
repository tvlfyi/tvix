use tvix_store::blobservice::SledBlobService;
use tvix_store::chunkservice::SledChunkService;
use tvix_store::directoryservice::SledDirectoryService;
use tvix_store::nar::NonCachingNARCalculationService;
use tvix_store::pathinfoservice::SledPathInfoService;
use tvix_store::proto::blob_service_server::BlobServiceServer;
use tvix_store::proto::directory_service_server::DirectoryServiceServer;
use tvix_store::proto::path_info_service_server::PathInfoServiceServer;
use tvix_store::proto::GRPCBlobServiceWrapper;
use tvix_store::proto::GRPCDirectoryServiceWrapper;
use tvix_store::proto::GRPCPathInfoServiceWrapper;

#[cfg(feature = "reflection")]
use tvix_store::proto::FILE_DESCRIPTOR_SET;

use clap::Parser;
use tonic::{transport::Server, Result};
use tracing::{info, Level};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[clap(long, short = 'l')]
    listen_address: Option<String>,

    #[clap(long)]
    log_level: Option<Level>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let listen_address = cli
        .listen_address
        .unwrap_or_else(|| "[::]:8000".to_string())
        .parse()
        .unwrap();

    let level = cli.log_level.unwrap_or(Level::INFO);
    let subscriber = tracing_subscriber::fmt().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber).ok();

    let mut server = Server::builder();

    let blob_service = SledBlobService::new("blobs.sled".into())?;
    let chunk_service = SledChunkService::new("chunks.sled".into())?;
    let directory_service = SledDirectoryService::new("directories.sled".into())?;
    let path_info_service = SledPathInfoService::new("pathinfo.sled".into())?;

    let nar_calculation_service = NonCachingNARCalculationService::new(
        blob_service.clone(),
        chunk_service.clone(),
        directory_service.clone(),
    );

    let mut router = server
        .add_service(BlobServiceServer::new(GRPCBlobServiceWrapper::new(
            blob_service,
            chunk_service,
        )))
        .add_service(DirectoryServiceServer::new(
            GRPCDirectoryServiceWrapper::from(directory_service),
        ))
        .add_service(PathInfoServiceServer::new(GRPCPathInfoServiceWrapper::new(
            path_info_service,
            nar_calculation_service,
        )));

    #[cfg(feature = "reflection")]
    {
        let reflection_svc = tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
            .build()?;
        router = router.add_service(reflection_svc);
    }

    info!("tvix-store listening on {}", listen_address);

    router.serve(listen_address).await?;

    Ok(())
}
