#[cfg(feature = "tonic")]
pub mod tonic;

#[cfg(feature = "reqwest")]
pub mod reqwest;

// TODO: Helper library for axum or another http server, see
// https://github.com/hseeberger/hello-tracing-rs/blob/main/hello-tracing-common/src/otel/http.rs
// as an example and we can reuse tonic::accept_trace fun, at least for a tower::ServiceBuilder
