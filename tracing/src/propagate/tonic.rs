#[cfg(feature = "otlp")]
use opentelemetry::{global, propagation::Injector};
#[cfg(feature = "otlp")]
use opentelemetry_http::HeaderExtractor;
#[cfg(feature = "otlp")]
use tracing_opentelemetry::OpenTelemetrySpanExt;

/// Trace context propagation: associate the current span with the otlp trace of the given request,
/// if any and valid. This only sets the parent trace if the otlp feature is also enabled.
pub fn accept_trace<B>(request: http::Request<B>) -> http::Request<B> {
    // we only extract and set a parent trace if otlp feature is enabled, otherwise this feature is
    // an noop and we return the request as is
    #[cfg(feature = "otlp")]
    {
        // Current context, if no or invalid data is received.
        let parent_context = global::get_text_map_propagator(|propagator| {
            propagator.extract(&HeaderExtractor(request.headers()))
        });
        tracing::Span::current().set_parent(parent_context);
    }
    request
}

#[cfg(feature = "otlp")]
struct MetadataInjector<'a>(&'a mut tonic::metadata::MetadataMap);

#[cfg(feature = "otlp")]
impl Injector for MetadataInjector<'_> {
    fn set(&mut self, key: &str, value: String) {
        use tonic::metadata::{MetadataKey, MetadataValue};
        use tracing::warn;

        match MetadataKey::from_bytes(key.as_bytes()) {
            Ok(key) => match MetadataValue::try_from(&value) {
                Ok(value) => {
                    self.0.insert(key, value);
                }
                Err(error) => warn!(value, error = format!("{error:#}"), "parse metadata value"),
            },
            Err(error) => warn!(key, error = format!("{error:#}"), "parse metadata key"),
        }
    }
}

/// Trace context propagation: send the trace context by injecting it into the metadata of the given
/// request. This only injects the current span if the otlp feature is also enabled.
#[allow(unused_mut)]
pub fn send_trace<T>(mut request: tonic::Request<T>) -> Result<tonic::Request<T>, tonic::Status> {
    #[cfg(feature = "otlp")]
    {
        global::get_text_map_propagator(|propagator| {
            let context = tracing::Span::current().context();
            propagator.inject_context(&context, &mut MetadataInjector(request.metadata_mut()))
        });
    }
    Ok(request)
}
