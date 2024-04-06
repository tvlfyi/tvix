use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint};

fn url_wants_wait_connect(url: &url::Url) -> bool {
    url.query_pairs()
        .filter(|(k, v)| k == "wait-connect" && v == "1")
        .count()
        > 0
}

/// Turn a [url::Url] to a [Channel] if it can be parsed successfully.
/// It supports the following schemes (and URLs):
///  - `grpc+http://[::1]:8000`, connecting over unencrypted HTTP/2 (h2c)
///  - `grpc+https://[::1]:8000`, connecting over encrypted HTTP/2
///  - `grpc+unix:/path/to/socket`, connecting to a unix domain socket
///
/// All URLs support adding `wait-connect=1` as a URL parameter, in which case
/// the connection is established lazily.
pub async fn channel_from_url(url: &url::Url) -> Result<Channel, self::Error> {
    match url.scheme() {
        "grpc+unix" => {
            if url.host_str().is_some() {
                return Err(Error::HostSetForUnixSocket());
            }

            let connector = tower::service_fn({
                let url = url.clone();
                move |_: tonic::transport::Uri| UnixStream::connect(url.path().to_string().clone())
            });

            // the URL doesn't matter
            let endpoint = Endpoint::from_static("http://[::]:50051");
            if url_wants_wait_connect(url) {
                Ok(endpoint.connect_with_connector(connector).await?)
            } else {
                Ok(endpoint.connect_with_connector_lazy(connector))
            }
        }
        _ => {
            // ensure path is empty, not supported with gRPC.
            if !url.path().is_empty() {
                return Err(Error::PathMayNotBeSet());
            }

            // Stringify the URL and remove the grpc+ prefix.
            // We can't use `url.set_scheme(rest)`, as it disallows
            // setting something http(s) that previously wasn't.
            let unprefixed_url_str = match url.to_string().strip_prefix("grpc+") {
                None => return Err(Error::MissingGRPCPrefix()),
                Some(url_str) => url_str.to_owned(),
            };

            // Use the regular tonic transport::Endpoint logic, but unprefixed_url_str,
            // as tonic doesn't know about grpc+http[s].
            let endpoint = Endpoint::try_from(unprefixed_url_str)?;
            if url_wants_wait_connect(url) {
                Ok(endpoint.connect().await?)
            } else {
                Ok(endpoint.connect_lazy())
            }
        }
    }
}

/// Errors occuring when trying to connect to a backend
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("grpc+ prefix is missing from URL")]
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

#[cfg(test)]
mod tests {
    use super::channel_from_url;
    use rstest::rstest;
    use url::Url;

    #[rstest]
    /// Correct scheme to connect to a unix socket.
    #[case::valid_unix_socket("grpc+unix:///path/to/somewhere", true)]
    /// Connecting with wait-connect set to 0 succeeds, as that's the default.
    #[case::valid_unix_socket_wait_connect_0("grpc+unix:///path/to/somewhere?wait-connect=0", true)]
    /// Connecting with wait-connect set to 1 fails, as the path doesn't exist.
    #[case::valid_unix_socket_wait_connect_1(
        "grpc+unix:///path/to/somewhere?wait-connect=1",
        false
    )]
    /// Correct scheme for unix socket, but setting a host too, which is invalid.
    #[case::invalid_unix_socket_and_host("grpc+unix://host.example/path/to/somewhere", false)]
    /// Correct scheme to connect to localhost, with port 12345
    #[case::valid_ipv6_localhost_port_12345("grpc+http://[::1]:12345", true)]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[case::valid_http_host_without_port("grpc+http://localhost", true)]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[case::valid_https_host_without_port("grpc+https://localhost", true)]
    /// Correct scheme to connect to localhost over http, but with additional path, which is invalid.
    #[case::invalid_host_and_path("grpc+http://localhost/some-path", false)]
    /// Connecting with wait-connect set to 0 succeeds, as that's the default.
    #[case::valid_host_wait_connect_0("grpc+http://localhost?wait-connect=0", true)]
    /// Connecting with wait-connect set to 1 fails, as the host doesn't exist.
    #[case::valid_host_wait_connect_1_fails("grpc+http://nonexist.invalid?wait-connect=1", false)]
    #[tokio::test]
    async fn test_from_addr_tokio(#[case] uri_str: &str, #[case] is_ok: bool) {
        let url = Url::parse(uri_str).expect("must parse");
        assert_eq!(channel_from_url(&url).await.is_ok(), is_ok)
    }
}
