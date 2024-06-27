use reqwest_tracing::{SpanBackendWithUrl, TracingMiddleware};

/// Returns a new tracing middleware which can be used with reqwest_middleware.
/// It will then write the `traceparent` in the header on the request and additionally records the
/// `url` into `http.url`.
///
/// If otlp feature is disabled, this will not insert a `traceparent` into the header. It will
/// basically function as a noop.
///
/// `traceparent` => https://www.w3.org/TR/trace-context/#trace-context-http-headers-format
pub fn tracing_middleware() -> TracingMiddleware<SpanBackendWithUrl> {
    TracingMiddleware::<SpanBackendWithUrl>::new()
}
