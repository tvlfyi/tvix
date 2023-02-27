use clap::Subcommand;
use data_encoding::BASE64;
use std::path::PathBuf;
use tracing_subscriber::prelude::*;
use tvix_store::blobservice::SledBlobService;
use tvix_store::chunkservice::SledChunkService;
use tvix_store::directoryservice::SledDirectoryService;
use tvix_store::import::import_path;
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
    /// Whether to log in JSON
    #[arg(long)]
    json: bool,

    #[arg(long)]
    log_level: Option<Level>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Runs the tvix-store daemon.
    Daemon {
        #[arg(long, short = 'l')]
        listen_address: Option<String>,
    },
    /// Imports a list of paths into the store (not using the daemon)
    Import {
        #[clap(value_name = "PATH")]
        paths: Vec<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // configure log settings
    let level = cli.log_level.unwrap_or(Level::INFO);

    let subscriber = tracing_subscriber::registry()
        .with(if cli.json {
            Some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(std::io::stdout.with_max_level(level))
                    .json(),
            )
        } else {
            None
        })
        .with(if !cli.json {
            Some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(std::io::stdout.with_max_level(level))
                    .pretty(),
            )
        } else {
            None
        });

    tracing::subscriber::set_global_default(subscriber).expect("Unable to set global subscriber");

    // initialize stores
    let mut blob_service = SledBlobService::new("blobs.sled".into())?;
    let mut chunk_service = SledChunkService::new("chunks.sled".into())?;
    let mut directory_service = SledDirectoryService::new("directories.sled".into())?;
    let path_info_service = SledPathInfoService::new("pathinfo.sled".into())?;

    match cli.command {
        Commands::Daemon { listen_address } => {
            let listen_address = listen_address
                .unwrap_or_else(|| "[::]:8000".to_string())
                .parse()
                .unwrap();

            let mut server = Server::builder();

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
        }
        Commands::Import { paths } => {
            for path in paths {
                let root_node = import_path(
                    &mut blob_service,
                    &mut chunk_service,
                    &mut directory_service,
                    &path,
                )?;

                match root_node {
                    tvix_store::proto::node::Node::Directory(directory_node) => {
                        info!(
                            path = ?path,
                            name = directory_node.name,
                            digest = BASE64.encode(&directory_node.digest),
                            "import successful",
                        )
                    }
                    tvix_store::proto::node::Node::File(file_node) => {
                        info!(
                            path = ?path,
                            name = file_node.name,
                            digest = BASE64.encode(&file_node.digest),
                            "import successful"
                        )
                    }
                    tvix_store::proto::node::Node::Symlink(symlink_node) => {
                        info!(
                            path = ?path,
                            name = symlink_node.name,
                            target = symlink_node.target,
                            "import successful"
                        )
                    }
                }
            }
        }
    };
    Ok(())
}
