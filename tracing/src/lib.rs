use indicatif::ProgressStyle;
use lazy_static::lazy_static;
use tokio::sync::{mpsc, oneshot};
use tracing::Level;
use tracing_indicatif::{filter::IndicatifFilter, writer, IndicatifLayer, IndicatifWriter};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

#[cfg(feature = "otlp")]
use opentelemetry::{trace::Tracer, KeyValue};
#[cfg(feature = "otlp")]
use opentelemetry_sdk::{
    propagation::TraceContextPropagator,
    resource::{ResourceDetector, SdkProvidedResourceDetector},
    trace::BatchConfigBuilder,
    Resource,
};
#[cfg(feature = "tracy")]
use tracing_tracy::TracyLayer;

pub mod propagate;

lazy_static! {
    pub static ref PB_PROGRESS_STYLE: ProgressStyle = ProgressStyle::with_template(
        "{span_child_prefix} {wide_msg} {bar:10} ({elapsed}) {pos:>7}/{len:7}"
    )
    .expect("invalid progress template");
    pub static ref PB_TRANSFER_STYLE: ProgressStyle = ProgressStyle::with_template(
        "{span_child_prefix} {wide_msg} {binary_bytes:>7}/{binary_total_bytes:7}@{decimal_bytes_per_sec} ({elapsed}) {bar:10} "
    )
    .expect("invalid progress template");
    pub static ref PB_SPINNER_STYLE: ProgressStyle = ProgressStyle::with_template(
        "{span_child_prefix}{spinner} {wide_msg} ({elapsed}) {pos:>7}/{len:7}"
    )
    .expect("invalid progress template");
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Init(#[from] tracing_subscriber::util::TryInitError),

    #[error(transparent)]
    MpscSend(#[from] mpsc::error::SendError<Option<oneshot::Sender<()>>>),

    #[error(transparent)]
    OneshotRecv(#[from] oneshot::error::RecvError),
}

#[derive(Clone)]
pub struct TracingHandle {
    tx: Option<mpsc::Sender<Option<oneshot::Sender<()>>>>,

    stdout_writer: IndicatifWriter<writer::Stdout>,
    stderr_writer: IndicatifWriter<writer::Stderr>,
}

impl TracingHandle {
    /// Returns a writer for [std::io::Stdout] that ensures its output will not be clobbered by
    /// active progress bars.
    ///
    /// Instead of `println!(...)` prefer `writeln!(handle.get_stdout_writer(), ...)`
    pub fn get_stdout_writer(&self) -> IndicatifWriter<writer::Stdout> {
        // clone is fine here because its only a wrapper over an `Arc`
        self.stdout_writer.clone()
    }

    /// Returns a writer for [std::io::Stderr] that ensures its output will not be clobbered by
    /// active progress bars.
    ///
    /// Instead of `println!(...)` prefer `writeln!(handle.get_stderr_writer(), ...)`.
    pub fn get_stderr_writer(&self) -> IndicatifWriter<writer::Stderr> {
        // clone is fine here because its only a wrapper over an `Arc`
        self.stderr_writer.clone()
    }

    /// This will flush possible attached tracing providers, e.g. otlp exported, if enabled.
    /// If there is none enabled this will result in a noop.
    ///
    /// It will not wait until the flush is complete, but you can pass in an oneshot::Sender which
    /// will receive a message once the flush is completed.
    pub async fn flush(&self, msg: Option<oneshot::Sender<()>>) -> Result<(), Error> {
        if let Some(tx) = &self.tx {
            Ok(tx.send(msg).await?)
        } else {
            // If we have a message passed in we need to notify the receiver
            if let Some(tx) = msg {
                let _ = tx.send(());
            }
            Ok(())
        }
    }

    /// This will flush all all attached tracing providers and will wait until the flush is completed.
    /// If no tracing providers like otlp are attached then this will be a noop.
    ///
    /// This should only be called on a regular shutdown.
    /// If you correctly need to shutdown tracing on ctrl_c use [force_shutdown](#method.force_shutdown)
    /// otherwise you will get otlp errors.
    pub async fn shutdown(&self) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.flush(Some(tx)).await?;
        rx.await?;
        Ok(())
    }

    /// This will flush all all attached tracing providers and will wait until the flush is completed.
    /// After this it will do some other necessary cleanup.
    /// If no tracing providers like otlp are attached then this will be a noop.
    ///
    /// This should only be used if the tool received an ctrl_c otherwise you will get otlp errors.
    /// If you need to shutdown tracing on a regular exit, you should use the [shutdown](#method.shutdown)
    /// method.
    pub async fn force_shutdown(&self) -> Result<(), Error> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.flush(Some(tx)).await?;
        rx.await?;

        #[cfg(feature = "otlp")]
        {
            // Because of a bug within otlp we currently have to use spawn_blocking otherwise
            // calling `shutdown_tracer_provider` can block forever. See
            // https://github.com/open-telemetry/opentelemetry-rust/issues/1395#issuecomment-1953280335
            //
            // This still throws an error, if the tool exits regularly: "OpenTelemetry trace error
            // occurred. oneshot canceled", but not having this leads to errors if we cancel with
            // ctrl_c.
            // So this should right now only be used on ctrl_c, for a regular exit use the
            // [shutdown](#shutdown) method
            let _ = tokio::task::spawn_blocking(move || {
                opentelemetry::global::shutdown_tracer_provider();
            })
            .await;
        }

        Ok(())
    }
}

pub struct TracingBuilder {
    level: Level,
    progess_bar: bool,

    #[cfg(feature = "otlp")]
    service_name: Option<&'static str>,
}

impl Default for TracingBuilder {
    fn default() -> Self {
        TracingBuilder {
            level: Level::INFO,
            progess_bar: false,

            #[cfg(feature = "otlp")]
            service_name: None,
        }
    }
}

impl TracingBuilder {
    /// Set the log level for all layers: stderr und otlp if configured. RUST_LOG still has a
    /// higher priority over this value.
    pub fn level(mut self, level: Level) -> TracingBuilder {
        self.level = level;
        self
    }

    #[cfg(feature = "otlp")]
    /// Enable otlp by setting a custom service_name
    pub fn enable_otlp(mut self, service_name: &'static str) -> TracingBuilder {
        self.service_name = Some(service_name);
        self
    }

    /// Enable progress bar layer, default is disabled
    pub fn enable_progressbar(mut self) -> TracingBuilder {
        self.progess_bar = true;
        self
    }

    /// This will setup tracing based on the configuration passed in.
    /// It will setup a stderr writer output layer and a EnvFilter based on the provided log
    /// level (RUST_LOG still has a higher priority over the configured value).
    /// The EnvFilter will be applied to all configured layers, also otlp.
    ///
    /// It will also configure otlp if the feature is enabled and a service_name was provided. It
    /// will then correctly setup a channel which is later used for flushing the provider.
    pub fn build(self) -> Result<TracingHandle, Error> {
        // Set up the tracing subscriber.
        let indicatif_layer = IndicatifLayer::new().with_progress_style(PB_SPINNER_STYLE.clone());
        let stdout_writer = indicatif_layer.get_stdout_writer();
        let stderr_writer = indicatif_layer.get_stderr_writer();
        let subscriber = tracing_subscriber::registry()
            .with(
                EnvFilter::builder()
                    .with_default_directive(self.level.into())
                    .from_env()
                    .expect("invalid RUST_LOG"),
            )
            .with(
                tracing_subscriber::fmt::Layer::new()
                    .with_writer(indicatif_layer.get_stderr_writer())
                    .compact(),
            )
            .with((self.progess_bar).then(|| {
                indicatif_layer.with_filter(
                    // only show progress for spans with indicatif.pb_show field being set
                    IndicatifFilter::new(false),
                )
            }));

        // Setup otlp if a service_name is configured
        #[cfg(feature = "otlp")]
        {
            if let Some(service_name) = self.service_name {
                // register a text map propagator for trace propagation
                opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

                let (tracer, tx) = gen_otlp_tracer(service_name.to_string());
                // Create a tracing layer with the configured tracer
                let layer = tracing_opentelemetry::layer().with_tracer(tracer);

                #[cfg(feature = "tracy")]
                {
                    subscriber
                        .with(TracyLayer::default())
                        .with(Some(layer))
                        .try_init()?;
                }

                #[cfg(not(feature = "tracy"))]
                {
                    subscriber.with(Some(layer)).try_init()?;
                }
                return Ok(TracingHandle {
                    tx: Some(tx),
                    stdout_writer,
                    stderr_writer,
                });
            }
        }
        #[cfg(feature = "tracy")]
        {
            subscriber.with(TracyLayer::default()).try_init()?;
        }
        #[cfg(not(feature = "tracy"))]
        {
            subscriber.try_init()?;
        }

        Ok(TracingHandle {
            tx: None,
            stdout_writer,
            stderr_writer,
        })
    }
}

/// Returns an OTLP tracer, and the TX part of a channel, which can be used
/// to request flushes (and signal back the completion of the flush).
#[cfg(feature = "otlp")]
fn gen_otlp_tracer(
    service_name: String,
) -> (
    impl Tracer + tracing_opentelemetry::PreSampledTracer,
    mpsc::Sender<Option<oneshot::Sender<()>>>,
) {
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(opentelemetry_otlp::new_exporter().tonic())
        .with_batch_config(
            BatchConfigBuilder::default()
                // the default values for `max_export_batch_size` is set to 512, which we will fill
                // pretty quickly, which will then result in an export. We want to make sure that
                // the export is only done once the schedule is met and not as soon as 512 spans
                // are collected.
                .with_max_export_batch_size(4096)
                // analog to default config `max_export_batch_size * 4`
                .with_max_queue_size(4096 * 4)
                // only force an export to the otlp collector every 10 seconds to reduce the amount
                // of error messages if an otlp collector is not available
                .with_scheduled_delay(std::time::Duration::from_secs(10))
                .build(),
        )
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
        .expect("Failed to install batch exporter using Tokio");

    // Trace provider is need for later use like flushing the provider.
    // Needs to be kept around for each message to rx we need to handle.
    let tracer_provider = tracer
        .provider()
        .expect("Failed to get the tracer provider");

    // Set up a channel for flushing trace providers later
    let (tx, mut rx) = mpsc::channel::<Option<oneshot::Sender<()>>>(16);

    // Spawning a task that listens on rx for any message. Once we receive a message we
    // correctly call flush on the tracer_provider.
    tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            // Because of a bug within otlp we currently have to use spawn_blocking
            // otherwise will calling `force_flush` block forever, especially if the
            // tool was closed with ctrl_c. See
            // https://github.com/open-telemetry/opentelemetry-rust/issues/1395#issuecomment-1953280335
            let _ = tokio::task::spawn_blocking({
                let tracer_provider = tracer_provider.clone();
                move || tracer_provider.force_flush()
            })
            .await;
            if let Some(tx) = m {
                let _ = tx.send(());
            }
        }
    });

    (tracer, tx)
}
