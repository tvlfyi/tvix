use clap::Parser;
use clap::Subcommand;

use futures::future::try_join_all;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_listener::Listener;
use tokio_listener::SystemOptions;
use tokio_listener::UserOptions;
use tonic::transport::Server;
use tracing::info;
use tracing::Level;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
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

#[cfg(feature = "otlp")]
use opentelemetry::KeyValue;
#[cfg(feature = "otlp")]
use opentelemetry_sdk::{
    resource::{ResourceDetector, SdkProvidedResourceDetector},
    trace::BatchConfig,
    Resource,
};

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

    /// Whether to configure OTLP. Set --otlp=false to disable.
    #[arg(long, default_missing_value = "true", default_value = "true", num_args(0..=1), require_equals(true), action(clap::ArgAction::Set))]
    otlp: bool,

    /// A global log level to use when printing logs.
    /// It's also possible to set `RUST_LOG` according to
    /// `tracing_subscriber::filter::EnvFilter`, which will always have
    /// priority.
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

        #[arg(long, env, default_value_t = false)]
        /// Whether to configure the mountpoint with allow_other.
        /// Requires /etc/fuse.conf to contain the `user_allow_other`
        /// option, configured via `programs.fuse.userAllowOther` on NixOS.
        allow_other: bool,

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

    // Set up the tracing subscriber.
    let subscriber = tracing_subscriber::registry()
        .with(
            cli.json.then_some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(std::io::stderr)
                    .json()
                    .with_filter(
                        EnvFilter::builder()
                            .with_default_directive(level.into())
                            .from_env()
                            .expect("invalid RUST_LOG"),
                    ),
            ),
        )
        .with(
            (!cli.json).then_some(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(std::io::stderr)
                    .pretty()
                    .with_filter(
                        EnvFilter::builder()
                            .with_default_directive(level.into())
                            .from_env()
                            .expect("invalid RUST_LOG"),
                    ),
            ),
        );

    // Add the otlp layer (when otlp is enabled, and it's not disabled in the CLI)
    // then init the registry.
    // If the feature is feature-flagged out, just init without adding the layer.
    // It's necessary to do this separately, as every with() call chains the
    // layer into the type of the registry.
    #[cfg(feature = "otlp")]
    {
        let subscriber = if cli.otlp {
            let tracer = opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_exporter(opentelemetry_otlp::new_exporter().tonic())
                .with_batch_config(BatchConfig::default())
                .with_trace_config(opentelemetry_sdk::trace::config().with_resource({
                    // use SdkProvidedResourceDetector.detect to detect resources,
                    // but replace the default service name with our default.
                    // https://github.com/open-telemetry/opentelemetry-rust/issues/1298
                    let resources =
                        SdkProvidedResourceDetector.detect(std::time::Duration::from_secs(0));
                    // SdkProvidedResourceDetector currently always sets
                    // `service.name`, but we don't like its default.
                    if resources.get("service.name".into()).unwrap() == "unknown_service".into() {
                        resources.merge(&Resource::new([KeyValue::new(
                            "service.name",
                            "tvix.store",
                        )]))
                    } else {
                        resources
                    }
                }))
                .install_batch(opentelemetry_sdk::runtime::Tokio)?;

            // Create a tracing layer with the configured tracer
            let layer = tracing_opentelemetry::layer().with_tracer(tracer);

            subscriber.with(Some(layer))
        } else {
            subscriber.with(None)
        };

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
                            if let Ok(name) = tvix_store::import::path_to_name(&path) {
                                let resp = tvix_store::import::import_path_as_nar_ca(
                                    &path,
                                    name,
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
            allow_other,
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

                FuseDaemon::new(fs, &dest, threads, allow_other)
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
