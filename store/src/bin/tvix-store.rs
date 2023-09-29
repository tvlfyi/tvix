use clap::Subcommand;
use data_encoding::BASE64;
use futures::future::try_join_all;
use nix_compat::store_path;
use nix_compat::store_path::StorePath;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use tokio::task::JoinHandle;
use tracing_subscriber::prelude::*;
use tvix_castore::blobservice;
use tvix_castore::directoryservice;
use tvix_castore::import;
use tvix_castore::proto::blob_service_server::BlobServiceServer;
use tvix_castore::proto::directory_service_server::DirectoryServiceServer;
use tvix_castore::proto::node::Node;
use tvix_castore::proto::GRPCBlobServiceWrapper;
use tvix_castore::proto::GRPCDirectoryServiceWrapper;
use tvix_castore::proto::NamedNode;
use tvix_store::listener::ListenerStream;
use tvix_store::pathinfoservice;
use tvix_store::proto::path_info_service_server::PathInfoServiceServer;
use tvix_store::proto::GRPCPathInfoServiceWrapper;
use tvix_store::proto::NarInfo;
use tvix_store::proto::PathInfo;

#[cfg(feature = "fs")]
use tvix_store::fs::TvixStoreFs;

#[cfg(feature = "fuse")]
use tvix_store::fs::fuse::FuseDaemon;

#[cfg(feature = "virtiofs")]
use tvix_store::fs::virtiofs::start_virtiofs_daemon;

#[cfg(feature = "tonic-reflection")]
use tvix_castore::proto::FILE_DESCRIPTOR_SET as CASTORE_FILE_DESCRIPTOR_SET;
#[cfg(feature = "tonic-reflection")]
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
    /// Imports a list of paths into the store, print the store path for each of them.
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

        /// Number of FUSE threads to spawn.
        #[arg(long, env, default_value_t = default_threads())]
        threads: usize,

        /// Whether to list elements at the root of the mount point.
        /// This is useful if your PathInfoService doesn't provide an
        /// (exhaustive) listing.
        #[clap(long, short, action)]
        list_root: bool,
    },
    /// Starts a tvix-store virtiofs daemon at the given socket path.
    #[cfg(feature = "virtiofs")]
    #[command(name = "virtiofs")]
    VirtioFs {
        #[clap(value_name = "PATH")]
        socket: PathBuf,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        blob_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        directory_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        path_info_service_addr: String,

        /// Whether to list elements at the root of the mount point.
        /// This is useful if your PathInfoService doesn't provide an
        /// (exhaustive) listing.
        #[clap(long, short, action)]
        list_root: bool,
    },
}

#[cfg(all(feature = "fuse", not(target_os = "macos")))]
fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.into())
        .unwrap_or(4)
}
// On MacFUSE only a single channel will receive ENODEV when the file system is
// unmounted and so all the other channels will block forever.
// See https://github.com/osxfuse/osxfuse/issues/974
#[cfg(all(feature = "fuse", target_os = "macos"))]
fn default_threads() -> usize {
    1
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
                    .with_writer(io::stderr.with_max_level(level))
                    .json(),
            )
        } else {
            None
        })
        .with(if !cli.json {
            Some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(io::stderr.with_max_level(level))
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

            #[cfg(feature = "tonic-reflection")]
            {
                let reflection_svc = tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(CASTORE_FILE_DESCRIPTOR_SET)
                    .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                    .build()?;
                router = router.add_service(reflection_svc);
            }

            info!("tvix-store listening on {}", listen_address);

            let listener = ListenerStream::bind(&listen_address).await?;

            router.serve_with_incoming(listener).await?;
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

                    let task: JoinHandle<io::Result<()>> = tokio::task::spawn(async move {
                        // Ingest the path into blob and directory service.
                        let root_node = import::ingest_path(
                            blob_service.clone(),
                            directory_service.clone(),
                            &path,
                        )
                        .await
                        .expect("failed to ingest path");

                        // Ask the PathInfoService for the NAR size and sha256
                        let root_node_copy = root_node.clone();
                        let path_info_service_clone = path_info_service.clone();
                        let (nar_size, nar_sha256) = path_info_service_clone
                            .calculate_nar(&root_node_copy)
                            .await?;

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
                            node: Some(tvix_castore::proto::Node {
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
                        let path_info = path_info_service.put(path_info).await?;

                        let node = path_info.node.unwrap().node.unwrap();

                        log_node(&node, &path);

                        println!(
                            "{}",
                            StorePath::from_bytes(node.get_name())
                                .unwrap()
                                .to_absolute_path()
                        );

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
            threads,
        } => {
            let blob_service = blobservice::from_addr(&blob_service_addr)?;
            let directory_service = directoryservice::from_addr(&directory_service_addr)?;
            let path_info_service = pathinfoservice::from_addr(
                &path_info_service_addr,
                blob_service.clone(),
                directory_service.clone(),
            )?;

            let mut fuse_daemon = tokio::task::spawn_blocking(move || {
                let f = TvixStoreFs::new(
                    blob_service,
                    directory_service,
                    path_info_service,
                    list_root,
                );
                info!("mounting tvix-store on {:?}", &dest);

                FuseDaemon::new(f, &dest, threads)
            })
            .await??;

            // grab a handle to unmount the file system, and register a signal
            // handler.
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.unwrap();
                info!("interrupt received, unmounting…");
                tokio::task::spawn_blocking(move || fuse_daemon.unmount()).await??;
                info!("unmount occured, terminating…");
                Ok::<_, io::Error>(())
            })
            .await??;
        }
        #[cfg(feature = "virtiofs")]
        Commands::VirtioFs {
            socket,
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
                let fs = TvixStoreFs::new(
                    blob_service,
                    directory_service,
                    path_info_service,
                    list_root,
                );
                info!("starting tvix-store virtiofs daemon on {:?}", &socket);

                start_virtiofs_daemon(fs, socket)
            })
            .await??;
        }
    };
    Ok(())
}

fn log_node(node: &Node, path: &Path) {
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
