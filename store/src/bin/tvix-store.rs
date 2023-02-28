use clap::Parser;
use clap::Subcommand;

use futures::future::try_join_all;
use tonic::transport::Server;
use tracing::info;
use tracing::Level;

use std::path::PathBuf;
use std::sync::Arc;
use tokio_listener::Listener;
use tokio_listener::SystemOptions;
use tokio_listener::UserOptions;

use tracing_subscriber::prelude::*;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use tvix_castore::proto::blob_service_server::BlobServiceServer;
use tvix_castore::proto::directory_service_server::DirectoryServiceServer;
use tvix_castore::proto::GRPCBlobServiceWrapper;
use tvix_castore::proto::GRPCDirectoryServiceWrapper;
use tvix_store::pathinfoservice::PathInfoService;
use tvix_store::proto::path_info_service_server::PathInfoServiceServer;
use tvix_store::proto::GRPCPathInfoServiceWrapper;

#[cfg(any(feature = "fuse", feature = "virtiofs"))]
use tvix_store::pathinfoservice::make_fs;

#[cfg(feature = "fuse")]
use tvix_castore::fs::fuse::FuseDaemon;

#[cfg(feature = "virtiofs")]
use tvix_castore::fs::virtiofs::start_virtiofs_daemon;

#[cfg(feature = "tonic-reflection")]
use tvix_castore::proto::FILE_DESCRIPTOR_SET as CASTORE_FILE_DESCRIPTOR_SET;
#[cfg(feature = "tonic-reflection")]
use tvix_store::proto::FILE_DESCRIPTOR_SET;

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
        .with(
            cli.json.then_some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(std::io::stderr.with_max_level(level))
                    .json(),
            ),
        )
        .with(
            (!cli.json).then_some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(std::io::stderr.with_max_level(level))
                    .pretty(),
            ),
        );

    // Add the otlp layer (when otlp is enabled), then init the registry.
    // Or if the feature is disabled, just init without adding the layer.
    // It's necessary to do this separately, as every with() call chains the
    // layer into the type of the registry.
    #[cfg(feature = "otlp")]
    {
        let opentelemetry_layer = {
            let otlp_exporter = opentelemetry_otlp::new_exporter().tonic();
            // TODO: re-add once https://github.com/open-telemetry/opentelemetry-rust/pull/1252 is solved.
            // let mut metadata = tonic::metadata::MetadataMap::new();
            // metadata.insert("service.name", "tvix.store".parse()?);
            // otlp_exporter.with_metadata(metadata),

            let tracer = opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_exporter(otlp_exporter)
                .install_batch(opentelemetry_sdk::runtime::Tokio)?;

            // Create a tracing layer with the configured tracer
            tracing_opentelemetry::layer().with_tracer(tracer)
        };

        let subscriber = subscriber.with(opentelemetry_layer);
        subscriber.try_init()?;
    }

    // Init the registry (when otlp is not enabled)
    #[cfg(not(feature = "otlp"))]
    {
        subscriber.try_init()?;
    }

    match cli.command {
        Commands::Daemon {
            listen_address,
            blob_service_addr,
            directory_service_addr,
            path_info_service_addr,
        } => {
            // initialize stores
            let (blob_service, directory_service, path_info_service) =
                tvix_store::utils::construct_services(
                    blob_service_addr,
                    directory_service_addr,
                    path_info_service_addr,
                )
                .await?;

            let listen_address = listen_address
                .unwrap_or_else(|| "[::]:8000".to_string())
                .parse()
                .unwrap();

            let mut server = Server::builder();

            #[allow(unused_mut)]
            let mut router = server
                .add_service(BlobServiceServer::new(GRPCBlobServiceWrapper::new(
                    blob_service,
                )))
                .add_service(DirectoryServiceServer::new(
                    GRPCDirectoryServiceWrapper::new(directory_service),
                ))
                .add_service(PathInfoServiceServer::new(GRPCPathInfoServiceWrapper::new(
                    Arc::from(path_info_service),
                )));

            #[cfg(feature = "tonic-reflection")]
            {
                let reflection_svc = tonic_reflection::server::Builder::configure()
                    .register_encoded_file_descriptor_set(CASTORE_FILE_DESCRIPTOR_SET)
                    .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
                    .build()?;
                router = router.add_service(reflection_svc);
            }

            info!(listen_address=%listen_address, "starting daemon");

            let listener = Listener::bind(
                &listen_address,
                &SystemOptions::default(),
                &UserOptions::default(),
            )
            .await?;

            router.serve_with_incoming(listener).await?;
        }
        Commands::Import {
            paths,
            blob_service_addr,
            directory_service_addr,
            path_info_service_addr,
        } => {
            // FUTUREWORK: allow flat for single files?
            let (blob_service, directory_service, path_info_service) =
                tvix_store::utils::construct_services(
                    blob_service_addr,
                    directory_service_addr,
                    path_info_service_addr,
                )
                .await?;

            // Arc the PathInfoService, as we clone it .
            let path_info_service: Arc<dyn PathInfoService> = path_info_service.into();

            let tasks = paths
                .into_iter()
                .map(|path| {
                    tokio::task::spawn({
                        let blob_service = blob_service.clone();
                        let directory_service = directory_service.clone();
                        let path_info_service = path_info_service.clone();

                        async move {
                            let resp = tvix_store::utils::import_path(
                                path,
                                blob_service,
                                directory_service,
                                path_info_service,
                            )
                            .await;
                            if let Ok(output_path) = resp {
                                // If the import was successful, print the path to stdout.
                                println!("{}", output_path.to_absolute_path());
                            }
                        }
                    })
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
            let (blob_service, directory_service, path_info_service) =
                tvix_store::utils::construct_services(
                    blob_service_addr,
                    directory_service_addr,
                    path_info_service_addr,
                )
                .await?;

            let mut fuse_daemon = tokio::task::spawn_blocking(move || {
                let fs = make_fs(
                    blob_service,
                    directory_service,
                    Arc::from(path_info_service),
                    list_root,
                );
                info!(mount_path=?dest, "mounting");

                FuseDaemon::new(fs, &dest, threads)
            })
            .await??;

            // grab a handle to unmount the file system, and register a signal
            // handler.
            tokio::spawn(async move {
                tokio::signal::ctrl_c().await.unwrap();
                info!("interrupt received, unmounting…");
                tokio::task::spawn_blocking(move || fuse_daemon.unmount()).await??;
                info!("unmount occured, terminating…");
                Ok::<_, std::io::Error>(())
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
            let (blob_service, directory_service, path_info_service) =
                tvix_store::utils::construct_services(
                    blob_service_addr,
                    directory_service_addr,
                    path_info_service_addr,
                )
                .await?;

            tokio::task::spawn_blocking(move || {
                let fs = make_fs(
                    blob_service,
                    directory_service,
                    Arc::from(path_info_service),
                    list_root,
                );
                info!(socket_path=?socket, "starting virtiofs-daemon");

                start_virtiofs_daemon(fs, socket)
            })
            .await??;
        }
    };
    Ok(())
}
