use clap::Subcommand;
use data_encoding::BASE64;
use futures::future::try_join_all;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::prelude::*;
use tvix_store::blobservice::BlobService;
use tvix_store::blobservice::GRPCBlobService;
use tvix_store::blobservice::SledBlobService;
use tvix_store::directoryservice::DirectoryService;
use tvix_store::directoryservice::GRPCDirectoryService;
use tvix_store::directoryservice::SledDirectoryService;
use tvix_store::pathinfoservice::GRPCPathInfoService;
use tvix_store::pathinfoservice::SledPathInfoService;
use tvix_store::proto::blob_service_client::BlobServiceClient;
use tvix_store::proto::blob_service_server::BlobServiceServer;
use tvix_store::proto::directory_service_client::DirectoryServiceClient;
use tvix_store::proto::directory_service_server::DirectoryServiceServer;
use tvix_store::proto::node::Node;
use tvix_store::proto::path_info_service_client::PathInfoServiceClient;
use tvix_store::proto::path_info_service_server::PathInfoServiceServer;
use tvix_store::proto::GRPCBlobServiceWrapper;
use tvix_store::proto::GRPCDirectoryServiceWrapper;
use tvix_store::proto::GRPCPathInfoServiceWrapper;
use tvix_store::TvixStoreIO;
use tvix_store::FUSE;

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
    /// Mounts a tvix-store at the given mountpoint
    #[cfg(feature = "fuse")]
    Mount {
        #[clap(value_name = "PATH")]
        dest: PathBuf,
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
                    .with_writer(io::stdout.with_max_level(level))
                    .json(),
            )
        } else {
            None
        })
        .with(if !cli.json {
            Some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(io::stdout.with_max_level(level))
                    .pretty(),
            )
        } else {
            None
        });

    tracing::subscriber::set_global_default(subscriber).expect("Unable to set global subscriber");

    match cli.command {
        Commands::Daemon { listen_address } => {
            // initialize stores
            let blob_service: Arc<dyn BlobService> =
                Arc::new(SledBlobService::new("blobs.sled".into())?);
            let directory_service: Arc<dyn DirectoryService> =
                Arc::new(SledDirectoryService::new("directories.sled".into())?);
            let path_info_service = SledPathInfoService::new(
                "pathinfo.sled".into(),
                blob_service.clone(),
                directory_service.clone(),
            )?;

            let listen_address = listen_address
                .unwrap_or_else(|| "[::]:8000".to_string())
                .parse()
                .unwrap();

            let mut server = Server::builder();

            #[allow(unused_mut)]
            let mut router = server
                .add_service(BlobServiceServer::new(GRPCBlobServiceWrapper::from(
                    blob_service,
                )))
                .add_service(DirectoryServiceServer::new(
                    GRPCDirectoryServiceWrapper::from(directory_service),
                ))
                .add_service(PathInfoServiceServer::new(
                    GRPCPathInfoServiceWrapper::from(path_info_service),
                ));

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
            let blob_service = GRPCBlobService::from_client(
                BlobServiceClient::connect("http://[::1]:8000").await?,
            );
            let directory_service = GRPCDirectoryService::from_client(
                DirectoryServiceClient::connect("http://[::1]:8000").await?,
            );
            let path_info_service_client =
                PathInfoServiceClient::connect("http://[::1]:8000").await?;
            let path_info_service =
                GRPCPathInfoService::from_client(path_info_service_client.clone());

            let io = Arc::new(TvixStoreIO::new(
                Arc::new(blob_service),
                Arc::new(directory_service),
                path_info_service,
            ));

            let tasks = paths
                .iter()
                .map(|path| {
                    let io_move = io.clone();
                    let path = path.clone();
                    let task: tokio::task::JoinHandle<Result<(), io::Error>> =
                        tokio::task::spawn_blocking(move || {
                            let path_info = io_move.import_path_with_pathinfo(&path)?;
                            print_node(&path_info.node.unwrap().node.unwrap(), &path);
                            Ok(())
                        });
                    task
                })
                .collect::<Vec<tokio::task::JoinHandle<Result<(), io::Error>>>>();

            try_join_all(tasks).await?;
        }
        #[cfg(feature = "fuse")]
        Commands::Mount { dest } => {
            let blob_service = GRPCBlobService::from_client(
                BlobServiceClient::connect("http://[::1]:8000").await?,
            );
            let directory_service = GRPCDirectoryService::from_client(
                DirectoryServiceClient::connect("http://[::1]:8000").await?,
            );
            let path_info_service_client =
                PathInfoServiceClient::connect("http://[::1]:8000").await?;
            let path_info_service =
                GRPCPathInfoService::from_client(path_info_service_client.clone());

            tokio::task::spawn_blocking(move || {
                let f = FUSE::new(path_info_service, directory_service, blob_service);
                fuser::mount2(f, &dest, &[])
            })
            .await??
        }
    };
    Ok(())
}

fn print_node(node: &Node, path: &Path) {
    match node {
        Node::Directory(directory_node) => {
            info!(
                path = ?path,
                name = directory_node.name,
                digest = BASE64.encode(&directory_node.digest),
                "import successful",
            )
        }
        Node::File(file_node) => {
            info!(
                path = ?path,
                name = file_node.name,
                digest = BASE64.encode(&file_node.digest),
                "import successful"
            )
        }
        Node::Symlink(symlink_node) => {
            info!(
                path = ?path,
                name = symlink_node.name,
                target = symlink_node.target,
                "import successful"
            )
        }
    }
}
