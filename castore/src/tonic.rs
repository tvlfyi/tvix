use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint};

fn url_wants_wait_connect(url: &url::Url) -> bool {
    url.query_pairs()
        .filter(|(k, v)| k == "wait-connect" && v == "1")
        .count()
        > 0
}

/// Turn a [url::Url] to a [Channel] if it can be parsed successfully.
/// It supports `grpc+unix:/path/to/socket`, as well as the regular schemes supported
/// by tonic, for example `grpc+http://[::1]:8000`.
/// It supports wait-connect=1 as a URL parameter, in which case we don't connect lazily.
pub async fn channel_from_url(url: &url::Url) -> Result<Channel, self::Error> {
    // Stringify the URL and remove the grpc+ prefix.
    // We can't use `url.set_scheme(rest)`, as it disallows
    // setting something http(s) that previously wasn't.
    let unprefixed_url_str = match url.to_string().strip_prefix("grpc+") {
        None => return Err(Error::MissingGRPCPrefix()),
        Some(url_str) => url_str.to_owned(),
    };

    if url.scheme() == "grpc+unix" {
        if url.host_str().is_some() {
            return Err(Error::HostSetForUnixSocket());
        }

        let connector = tower::service_fn({
            let url = url.clone();
            move |_: tonic::transport::Uri| UnixStream::connect(url.path().to_string().clone())
        });

        let channel = if url_wants_wait_connect(url) {
            Endpoint::from_static("http://[::]:50051")
                .connect_with_connector(connector)
                .await?
        } else {
            Endpoint::from_static("http://[::]:50051").connect_with_connector_lazy(connector)
        };

        return Ok(channel);
    }

    // ensure path is empty, not supported with gRPC.
    if !url.path().is_empty() {
        return Err(Error::PathMayNotBeSet());
    }

    // Use the regular tonic transport::Endpoint logic, but unprefixed_url_str,
    // as tonic doesn't know about grpc+http[s].
    let endpoint = Endpoint::try_from(unprefixed_url_str)?;
    let channel = if url_wants_wait_connect(url) {
        endpoint.connect().await?
    } else {
        endpoint.connect_lazy()
    };

    Ok(channel)
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
    use test_case::test_case;
    use url::Url;

    /// Correct scheme to connect to a unix socket.
    #[test_case("grpc+unix:///path/to/somewhere", true; "grpc valid unix socket")]
    /// Connecting with wait-connect set to 0 succeeds, as that's the default.
    #[test_case("grpc+unix:///path/to/somewhere?wait-connect=0", true; "grpc valid unix wait-connect=0")]
    /// Connecting with wait-connect set to 1 fails, as the path doesn't exist.
    #[test_case("grpc+unix:///path/to/somewhere?wait-connect=1", false; "grpc valid unix wait-connect=1")]
    /// Correct scheme for unix socket, but setting a host too, which is invalid.
    #[test_case("grpc+unix://host.example/path/to/somewhere", false; "grpc invalid unix socket and host")]
    /// Correct scheme to connect to localhost, with port 12345
    #[test_case("grpc+http://[::1]:12345", true; "grpc valid IPv6 localhost port 12345")]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[test_case("grpc+http://localhost", true; "grpc valid http host without port")]
    /// Correct scheme to connect to localhost over http, without specifying a port.
    #[test_case("grpc+https://localhost", true; "grpc valid https host without port")]
    /// Correct scheme to connect to localhost over http, but with additional path, which is invalid.
    #[test_case("grpc+http://localhost/some-path", false; "grpc valid invalid host and path")]
    /// Connecting with wait-connect set to 0 succeeds, as that's the default.
    #[test_case("grpc+http://localhost?wait-connect=0", true; "grpc valid host wait-connect=0")]
    /// Connecting with wait-connect set to 1 fails, as the host doesn't exist.
    #[test_case("grpc+http://nonexist.invalid?wait-connect=1", false; "grpc valid host wait-connect=1")]
    #[tokio::test]
    async fn test_from_addr_tokio(uri_str: &str, is_ok: bool) {
        let url = Url::parse(uri_str).expect("must parse");
        assert_eq!(channel_from_url(&url).await.is_ok(), is_ok)
    }
}
