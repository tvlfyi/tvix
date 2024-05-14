use clap::Parser;
use nar_bridge::AppState;
use tracing::info;

/// Expose the Nix HTTP Binary Cache protocol for a tvix-store.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
    blob_service_addr: String,

    #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
    directory_service_addr: String,

    #[arg(long, env, default_value = "grpc+http://[::1]:8000")]
    path_info_service_addr: String,

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
        tvix_store::utils::construct_services(
            cli.blob_service_addr,
            cli.directory_service_addr,
            cli.path_info_service_addr,
        )
        .await?;

    let state = AppState::new(blob_service, directory_service, path_info_service.into());

    let app = nar_bridge::gen_router(cli.priority).with_state(state);

    let listen_address = &cli.listen_args.listen_address.unwrap_or_else(|| {
        "[::]:8000"
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
