use clap::Parser;
use clap::Subcommand;

use futures::future::try_join_all;
use futures::StreamExt;
use futures::TryStreamExt;
use indicatif::ProgressStyle;
use nix_compat::path_info::ExportedPathInfo;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_listener::Listener;
use tokio_listener::SystemOptions;
use tokio_listener::UserOptions;
use tonic::transport::Server;
use tracing::info;
use tracing::info_span;
use tracing::instrument;
use tracing::Level;
use tracing::Span;
use tracing_indicatif::filter::IndicatifFilter;
use tracing_indicatif::{span_ext::IndicatifSpanExt, IndicatifLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};
use tvix_castore::import::fs::ingest_path;
use tvix_store::nar::NarCalculationService;
use tvix_store::proto::NarInfo;
use tvix_store::proto::PathInfo;

use tvix_castore::proto::blob_service_server::BlobServiceServer;
use tvix_castore::proto::directory_service_server::DirectoryServiceServer;
use tvix_castore::proto::GRPCBlobServiceWrapper;
use tvix_castore::proto::GRPCDirectoryServiceWrapper;
use tvix_store::pathinfoservice::PathInfoService;
use tvix_store::proto::path_info_service_server::PathInfoServiceServer;
use tvix_store::proto::GRPCPathInfoServiceWrapper;

use lazy_static::lazy_static;

// FUTUREWORK: move this to tracing crate
lazy_static! {
    pub static ref PB_PROGRESS_STYLE: ProgressStyle = ProgressStyle::with_template(
        "{span_child_prefix}{bar:30} {wide_msg} [{elapsed_precise}]  {pos:>7}/{len:7}"
    )
    .expect("invalid progress template");
    pub static ref PB_SPINNER_STYLE: ProgressStyle = ProgressStyle::with_template(
        "{span_child_prefix}{spinner} {wide_msg} [{elapsed_precise}]  {pos:>7}/{len:7}"
    )
    .expect("invalid progress template");
}

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

        #[arg(
            long,
            env,
            default_value = "objectstore+file:///var/lib/tvix-store/blobs.object_store"
        )]
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

    /// Copies a list of store paths on the system into tvix-store.
    Copy {
        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        blob_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        directory_service_addr: String,

        #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
        path_info_service_addr: String,

        /// A path pointing to a JSON file produced by the Nix
        /// `__structuredAttrs` containing reference graph information provided
        /// by the `exportReferencesGraph` feature.
        ///
        /// This can be used to invoke tvix-store inside a Nix derivation
        /// copying to a Tvix store (or outside, if the JSON file is copied
        /// out).
        ///
        /// Currently limited to the `closure` key inside that JSON file.
        #[arg(value_name = "NIX_ATTRS_JSON_FILE", env = "NIX_ATTRS_JSON_FILE")]
        reference_graph_path: PathBuf,
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

        #[arg(long, default_value_t = true)]
        /// Whether to expose blob and directory digests as extended attributes.
        show_xattr: bool,
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

        #[arg(long, default_value_t = true)]
        /// Whether to expose blob and directory digests as extended attributes.
        show_xattr: bool,
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
#[instrument(fields(indicatif.pb_show=1))]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // configure log settings
    let level = cli.log_level.unwrap_or(Level::INFO);

    let indicatif_layer = IndicatifLayer::new().with_progress_style(PB_SPINNER_STYLE.clone());

    // Set up the tracing subscriber.
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::new()
                .with_writer(indicatif_layer.get_stderr_writer())
                .compact()
                .with_filter(
                    EnvFilter::builder()
                        .with_default_directive(level.into())
                        .from_env()
                        .expect("invalid RUST_LOG"),
                ),
        )
        .with(indicatif_layer.with_filter(
            // only show progress for spans with indicatif.pb_show field being set
            IndicatifFilter::new(false),
        ));

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
            let (blob_service, directory_service, path_info_service, nar_calculation_service) =
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
                    nar_calculation_service,
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
            let (blob_service, directory_service, path_info_service, nar_calculation_service) =
                tvix_store::utils::construct_services(
                    blob_service_addr,
                    directory_service_addr,
                    path_info_service_addr,
                )
                .await?;

            // Arc PathInfoService and NarCalculationService, as we clone it .
            let path_info_service: Arc<dyn PathInfoService> = path_info_service.into();
            let nar_calculation_service: Arc<dyn NarCalculationService> =
                nar_calculation_service.into();

            let root_span = {
                let s = Span::current();
                s.pb_set_style(&PB_PROGRESS_STYLE);
                s.pb_set_message("Importing paths");
                s.pb_set_length(paths.len() as u64);
                s.pb_start();
                s
            };

            let tasks = paths
                .into_iter()
                .map(|path| {
                    let paths_span = root_span.clone();
                    tokio::task::spawn({
                        let blob_service = blob_service.clone();
                        let directory_service = directory_service.clone();
                        let path_info_service = path_info_service.clone();
                        let nar_calculation_service = nar_calculation_service.clone();

                        async move {
                            if let Ok(name) = tvix_store::import::path_to_name(&path) {
                                let resp = tvix_store::import::import_path_as_nar_ca(
                                    &path,
                                    name,
                                    blob_service,
                                    directory_service,
                                    path_info_service,
                                    nar_calculation_service,
                                )
                                .await;
                                if let Ok(output_path) = resp {
                                    // If the import was successful, print the path to stdout.
                                    println!("{}", output_path.to_absolute_path());
                                }
                            }
                            paths_span.pb_inc(1);
                        }
                    })
                })
                .collect::<Vec<_>>();

            try_join_all(tasks).await?;
        }
        Commands::Copy {
            blob_service_addr,
            directory_service_addr,
            path_info_service_addr,
            reference_graph_path,
        } => {
            let (blob_service, directory_service, path_info_service, _nar_calculation_service) =
                tvix_store::utils::construct_services(
                    blob_service_addr,
                    directory_service_addr,
                    path_info_service_addr,
                )
                .await?;

            // Parse the file at reference_graph_path.
            let reference_graph_json = tokio::fs::read(&reference_graph_path).await?;

            #[derive(Deserialize, Serialize)]
            struct ReferenceGraph<'a> {
                #[serde(borrow)]
                closure: Vec<ExportedPathInfo<'a>>,
            }

            let reference_graph: ReferenceGraph<'_> =
                serde_json::from_slice(reference_graph_json.as_slice())?;

            // Arc the PathInfoService, as we clone it .
            let path_info_service: Arc<dyn PathInfoService> = path_info_service.into();

            let lookups_span = info_span!(
                "lookup pathinfos",
                "indicatif.pb_show" = tracing::field::Empty
            );
            lookups_span.pb_set_length(reference_graph.closure.len() as u64);
            lookups_span.pb_set_style(&PB_PROGRESS_STYLE);
            lookups_span.pb_start();

            // From our reference graph, lookup all pathinfos that might exist.
            let elems: Vec<_> = futures::stream::iter(reference_graph.closure)
                .map(|elem| {
                    let path_info_service = path_info_service.clone();
                    async move {
                        let resp = path_info_service
                            .get(*elem.path.digest())
                            .await
                            .map(|resp| (elem, resp));

                        Span::current().pb_inc(1);
                        resp
                    }
                })
                .buffer_unordered(50)
                // Filter out all that are already uploaded.
                // TODO: check if there's a better combinator for this
                .try_filter_map(|(elem, path_info)| {
                    std::future::ready(if path_info.is_none() {
                        Ok(Some(elem))
                    } else {
                        Ok(None)
                    })
                })
                .try_collect()
                .await?;

            // Run ingest_path on all of them.
            let uploads: Vec<_> = futures::stream::iter(elems)
                .map(|elem| {
                    // Map to a future returning the root node, alongside with the closure info.
                    let blob_service = blob_service.clone();
                    let directory_service = directory_service.clone();
                    async move {
                        // Ingest the given path.

                        ingest_path(
                            blob_service,
                            directory_service,
                            PathBuf::from(elem.path.to_absolute_path()),
                        )
                        .await
                        .map(|root_node| (elem, root_node))
                    }
                })
                .buffer_unordered(10)
                .try_collect()
                .await?;

            // Insert them into the PathInfoService.
            // FUTUREWORK: do this properly respecting the reference graph.
            for (elem, root_node) in uploads {
                // Create and upload a PathInfo pointing to the root_node,
                // annotated with information we have from the reference graph.
                let path_info = PathInfo {
                    node: Some(tvix_castore::proto::Node {
                        node: Some(root_node),
                    }),
                    references: Vec::from_iter(
                        elem.references.iter().map(|e| e.digest().to_vec().into()),
                    ),
                    narinfo: Some(NarInfo {
                        nar_size: elem.nar_size,
                        nar_sha256: elem.nar_sha256.to_vec().into(),
                        signatures: vec![],
                        reference_names: Vec::from_iter(
                            elem.references.iter().map(|e| e.to_string()),
                        ),
                        deriver: None,
                        ca: None,
                    }),
                };

                path_info_service.put(path_info).await?;
            }
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
            show_xattr,
        } => {
            let (blob_service, directory_service, path_info_service, _nar_calculation_service) =
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
                    show_xattr,
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
            show_xattr,
        } => {
            let (blob_service, directory_service, path_info_service, _nar_calculation_service) =
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
                    show_xattr,
                );
                info!(socket_path=?socket, "starting virtiofs-daemon");

                start_virtiofs_daemon(fs, socket)
            })
            .await??;
        }
    };
    Ok(())
}
