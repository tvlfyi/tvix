use clap::Subcommand;
use data_encoding::BASE64;
use futures::future::try_join_all;
use nix_compat::store_path;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use tracing_subscriber::prelude::*;
use tvix_store::blobservice;
use tvix_store::directoryservice;
use tvix_store::import;
use tvix_store::pathinfoservice;
use tvix_store::proto::blob_service_server::BlobServiceServer;
use tvix_store::proto::directory_service_server::DirectoryServiceServer;
use tvix_store::proto::node::Node;
use tvix_store::proto::path_info_service_server::PathInfoServiceServer;
use tvix_store::proto::GRPCBlobServiceWrapper;
use tvix_store::proto::GRPCDirectoryServiceWrapper;
use tvix_store::proto::GRPCPathInfoServiceWrapper;
use tvix_store::proto::NarInfo;
use tvix_store::proto::PathInfo;
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

        #[arg(long, env, default_value = "sled:///var/lib/tvix-store/blobs.sled")]
        blob_service_addr: String,

        #[arg(
            long,
            env,
            default_value = "sled:///var/lib/tvix-store/directories.sled"
        )]
        directory_service_addr: String,

        #[arg(long, env, default_value = "sled:///var/lib/tvix-store/pathinfo.sled")]
        path_info_service_addr: String,
    },
    /// Imports a list of paths into the store.
    Import {
        #[clap(value_name = "PATH")]
        paths: Vec<PathBuf>,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        blob_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        directory_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        path_info_service_addr: String,
    },
    /// Mounts a tvix-store at the given mountpoint
    #[cfg(feature = "fuse")]
    Mount {
        #[clap(value_name = "PATH")]
        dest: PathBuf,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        blob_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        directory_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        path_info_service_addr: String,

        /// Whether to list elements at the root of the mount point.
        #[clap(long, short, action)]
        list_root: bool,
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
        Commands::Daemon {
            listen_address,
            blob_service_addr,
            directory_service_addr,
            path_info_service_addr,
        } => {
            // initialize stores
            let blob_service = blobservice::from_addr(&blob_service_addr)?;
            let directory_service = directoryservice::from_addr(&directory_service_addr)?;
            let path_info_service = pathinfoservice::from_addr(
                &path_info_service_addr,
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
        Commands::Import {
            paths,
            blob_service_addr,
            directory_service_addr,
            path_info_service_addr,
        } => {
            // FUTUREWORK: allow flat for single files?
            let blob_service = blobservice::from_addr(&blob_service_addr)?;
            let directory_service = directoryservice::from_addr(&directory_service_addr)?;
            let path_info_service = pathinfoservice::from_addr(
                &path_info_service_addr,
                blob_service.clone(),
                directory_service.clone(),
            )?;

            let tasks = paths
                .into_iter()
                .map(|path| {
                    let blob_service = blob_service.clone();
                    let directory_service = directory_service.clone();
                    let path_info_service = path_info_service.clone();

                    let task = tokio::task::spawn_blocking(move || -> io::Result<()> {
                        // Ingest the path into blob and directory service.
                        let root_node = import::ingest_path(
                            blob_service.clone(),
                            directory_service.clone(),
                            &path,
                        )
                        .expect("failed to ingest path");

                        // Ask the PathInfoService for the NAR size and sha256
                        let (nar_size, nar_sha256) = path_info_service.calculate_nar(&root_node)?;

                        // TODO: make a path_to_name helper function?
                        let name = path
                            .file_name()
                            .expect("path must not be ..")
                            .to_str()
                            .expect("path must be valid unicode");

                        let output_path = store_path::build_nar_based_store_path(&nar_sha256, name);

                        // assemble a new root_node with a name that is derived from the nar hash.
                        let root_node =
                            root_node.rename(output_path.to_string().into_bytes().into());

                        // assemble the [crate::proto::PathInfo] object.
                        let path_info = PathInfo {
                            node: Some(tvix_store::proto::Node {
                                node: Some(root_node),
                            }),
                            // There's no reference scanning on path contents ingested like this.
                            references: vec![],
                            narinfo: Some(NarInfo {
                                nar_size,
                                nar_sha256: nar_sha256.to_vec().into(),
                                signatures: vec![],
                                reference_names: vec![],
                            }),
                        };

                        // put into [PathInfoService], and return the PathInfo that we get back
                        // from there (it might contain additional signatures).
                        let path_info = path_info_service.put(path_info)?;

                        print_node(&path_info.node.unwrap().node.unwrap(), &path);
                        Ok(())
                    });
                    task
                })
                .collect::<Vec<_>>();

            try_join_all(tasks).await?;
        }
        #[cfg(feature = "fuse")]
        Commands::Mount {
            dest,
            blob_service_addr,
            directory_service_addr,
            path_info_service_addr,
            list_root,
        } => {
            let blob_service = blobservice::from_addr(&blob_service_addr)?;
            let directory_service = directoryservice::from_addr(&directory_service_addr)?;
            let path_info_service = pathinfoservice::from_addr(
                &path_info_service_addr,
                blob_service.clone(),
                directory_service.clone(),
            )?;

            tokio::task::spawn_blocking(move || {
                let f = FUSE::new(
                    blob_service,
                    directory_service,
                    path_info_service,
                    list_root,
                );
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
                name = ?directory_node.name,
                digest = BASE64.encode(&directory_node.digest),
                "import successful",
            )
        }
        Node::File(file_node) => {
            info!(
                path = ?path,
                name = ?file_node.name,
                digest = BASE64.encode(&file_node.digest),
                "import successful"
            )
        }
        Node::Symlink(symlink_node) => {
            info!(
                path = ?path,
                name = ?symlink_node.name,
                target = ?symlink_node.target,
                "import successful"
            )
        }
    }
}
