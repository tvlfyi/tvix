use tvix_store::proto::blob_service_server::BlobServiceServer;
use tvix_store::proto::directory_service_server::DirectoryServiceServer;
use tvix_store::proto::path_info_service_server::PathInfoServiceServer;

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

    let blob_service = tvix_store::dummy_blob_service::DummyBlobService {};
    let directory_service = tvix_store::dummy_directory_service::DummyDirectoryService {};
    let path_info_service = tvix_store::dummy_path_info_service::DummyPathInfoService {};

    let mut router = server
        .add_service(BlobServiceServer::new(blob_service))
        .add_service(DirectoryServiceServer::new(directory_service))
        .add_service(PathInfoServiceServer::new(path_info_service));

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
