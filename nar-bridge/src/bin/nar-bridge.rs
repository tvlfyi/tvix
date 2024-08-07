use clap::Parser;
use mimalloc::MiMalloc;
use nar_bridge::AppState;
use tower::ServiceBuilder;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use tracing::info;
use tvix_store::utils::ServiceUrlsGrpc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

/// Expose the Nix HTTP Binary Cache protocol for a tvix-store.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[clap(flatten)]
    service_addrs: ServiceUrlsGrpc,

    /// The priority to announce at the `nix-cache-info` endpoint.
    /// A lower number means it's *more preferred.
    #[arg(long, env, default_value_t = 39)]
    priority: u64,

    /// The address to listen on.
    #[clap(flatten)]
    listen_args: tokio_listener::ListenerAddressLFlag,

    #[cfg(feature = "otlp")]
    /// Whether to configure OTLP. Set --otlp=false to disable.
    #[arg(long, default_missing_value = "true", default_value = "true", num_args(0..=1), require_equals(true), action(clap::ArgAction::Set))]
    otlp: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let cli = Cli::parse();

    let _tracing_handle = {
        #[allow(unused_mut)]
        let mut builder = tvix_tracing::TracingBuilder::default();
        #[cfg(feature = "otlp")]
        {
            if cli.otlp {
                builder = builder.enable_otlp("tvix.store");
            }
        }
        builder.build()?
    };

    // initialize stores
    let (blob_service, directory_service, path_info_service, _nar_calculation_service) =
        tvix_store::utils::construct_services(cli.service_addrs).await?;

    let state = AppState::new(blob_service, directory_service, path_info_service);

    let app = nar_bridge::gen_router(cli.priority)
        .layer(
            ServiceBuilder::new()
                .layer(
                    TraceLayer::new_for_http().make_span_with(
                        DefaultMakeSpan::new()
                            .level(tracing::Level::INFO)
                            .include_headers(true),
                    ),
                )
                .map_request(tvix_tracing::propagate::axum::accept_trace),
        )
        .with_state(state);

    let listen_address = &cli.listen_args.listen_address.unwrap_or_else(|| {
        "[::]:9000"
            .parse()
            .expect("invalid fallback listen address")
    });

    let listener = tokio_listener::Listener::bind(
        listen_address,
        &Default::default(),
        &cli.listen_args.listener_options,
    )
    .await?;

    info!(listen_address=%listen_address, "starting daemon");

    tokio_listener::axum07::serve(
        listener,
        app.into_make_service_with_connect_info::<tokio_listener::SomeSocketAddrClonable>(),
    )
    .await?;

    Ok(())
}
