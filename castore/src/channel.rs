use tokio::net::UnixStream;
use tonic::transport::Channel;

/// Turn a [url::Url] to a [Channel] if it can be parsed successfully.
/// It supports `grpc+unix:/path/to/socket`,
/// as well as the regular schemes supported by tonic, prefixed with grpc+,
/// for example `grpc+http://[::1]:8000`.
pub fn from_url(url: &url::Url) -> Result<Channel, self::Error> {
    // Start checking for the scheme to start with grpc+.
    // If it doesn't start with that, bail out.
    match url.scheme().strip_prefix("grpc+") {
        None => Err(Error::MissingGRPCPrefix()),
        Some(rest) => {
            if rest == "unix" {
                if url.host_str().is_some() {
                    return Err(Error::HostSetForUnixSocket());
                }

                let url = url.clone();
                Ok(
                    tonic::transport::Endpoint::from_static("http://[::]:50051") // doesn't matter
                        .connect_with_connector_lazy(tower::service_fn(
                            move |_: tonic::transport::Uri| {
                                UnixStream::connect(url.path().to_string().clone())
                            },
                        )),
                )
            } else {
                // ensure path is empty, not supported with gRPC.
                if !url.path().is_empty() {
                    return Err(Error::PathMayNotBeSet());
                }

                // Stringify the URL and remove the grpc+ prefix.
                // We can't use `url.set_scheme(rest)`, as it disallows
                // setting something http(s) that previously wasn't.
                let url = url.to_string().strip_prefix("grpc+").unwrap().to_owned();

                // Use the regular tonic transport::Endpoint logic to
                Ok(tonic::transport::Endpoint::try_from(url)
                    .unwrap()
                    .connect_lazy())
            }
        }
    }
}

/// Errors occuring when trying to connect to a backend
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("grpc+ prefix is missing from Url")]
    MissingGRPCPrefix(),

    #[error("host may not be set for unix domain sockets")]
    HostSetForUnixSocket(),

    #[error("path may not be set")]
    PathMayNotBeSet(),

    #[error("transport error: {0}")]
    TransportError(tonic::transport::Error),
}

impl From<tonic::transport::Error> for Error {
    fn from(value: tonic::transport::Error) -> Self {
        Self::TransportError(value)
    }
}
