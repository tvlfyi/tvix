use indicatif::ProgressStyle;
use lazy_static::lazy_static;
use tracing::Level;
use tracing_indicatif::{filter::IndicatifFilter, IndicatifLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

#[cfg(feature = "otlp")]
use opentelemetry::KeyValue;
#[cfg(feature = "otlp")]
use opentelemetry_sdk::{
    resource::{ResourceDetector, SdkProvidedResourceDetector},
    trace::BatchConfig,
    Resource,
};

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

// using a macro_rule here because of the complex return type
macro_rules! init_base_subscriber {
    ($level: expr) => {{
        let indicatif_layer = IndicatifLayer::new().with_progress_style(PB_SPINNER_STYLE.clone());

        // Set up the tracing subscriber.
        tracing_subscriber::registry()
            .with(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(indicatif_layer.get_stderr_writer())
                    .compact()
                    .with_filter(
                        EnvFilter::builder()
                            .with_default_directive($level.into())
                            .from_env()
                            .expect("invalid RUST_LOG"),
                    ),
            )
            .with(indicatif_layer.with_filter(
                // only show progress for spans with indicatif.pb_show field being set
                IndicatifFilter::new(false),
            ))
    }};
}

pub fn init(level: Level) -> Result<(), tracing_subscriber::util::TryInitError> {
    init_base_subscriber!(level).try_init()
}

#[cfg(feature = "otlp")]
pub fn init_with_otlp(
    level: Level,
    service_name: &'static str,
) -> Result<(), tracing_subscriber::util::TryInitError> {
    let subscriber = init_base_subscriber!(level);

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .with_batch_config(BatchConfig::default())
        .with_trace_config(opentelemetry_sdk::trace::config().with_resource({
            // use SdkProvidedResourceDetector.detect to detect resources,
            // but replace the default service name with our default.
            // https://github.com/open-telemetry/opentelemetry-rust/issues/1298
            let resources = SdkProvidedResourceDetector.detect(std::time::Duration::from_secs(0));
            // SdkProvidedResourceDetector currently always sets
            // `service.name`, but we don't like its default.
            if resources.get("service.name".into()).unwrap() == "unknown_service".into() {
                resources.merge(&Resource::new([KeyValue::new(
                    "service.name",
                    service_name,
                )]))
            } else {
                resources
            }
        }))
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        .expect("Failed to install tokio runtime");

    // Create a tracing layer with the configured tracer
    let layer = tracing_opentelemetry::layer().with_tracer(tracer);

    subscriber.with(Some(layer)).try_init()
}
