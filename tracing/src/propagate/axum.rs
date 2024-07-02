#[cfg(feature = "otlp")]
use opentelemetry::{global, propagation::Extractor};
#[cfg(feature = "otlp")]
use tracing_opentelemetry::OpenTelemetrySpanExt;

// TODO: accept_trace can be shared with tonic, as soon as tonic upstream has a release with
// support for axum07. Latest master already has support for axum07 but there is not release yet:
// https://github.com/hyperium/tonic/pull/1740

/// Trace context propagation: associate the current span with the otlp trace of the given request,
/// if any and valid. This only sets the parent trace if the otlp feature is also enabled.
pub fn accept_trace<B>(request: axum::http::Request<B>) -> axum::http::Request<B> {
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

/// Helper for extracting headers from HTTP Requests. This is used for OpenTelemetry context
/// propagation over HTTP.
#[cfg(feature = "otlp")]
struct HeaderExtractor<'a>(&'a axum::http::HeaderMap);

#[cfg(feature = "otlp")]
impl<'a> Extractor for HeaderExtractor<'a> {
    /// Get a value for a key from the HeaderMap.  If the value is not valid ASCII, returns None.
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| {
            let s = v.to_str();
            if let Err(ref error) = s {
                tracing::warn!(%error, ?v, "cannot convert header value to ASCII")
            };
            s.ok()
        })
    }

    /// Collect all the keys from the HeaderMap.
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}
