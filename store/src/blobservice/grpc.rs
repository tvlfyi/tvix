use super::{naive_seeker::NaiveSeeker, BlobReader, BlobService, BlobWriter};
use crate::{proto, B3Digest};
use futures::sink::SinkExt;
use futures::TryFutureExt;
use std::{
    collections::VecDeque,
    io::{self},
    pin::pin,
    task::Poll,
};
use tokio::io::AsyncWriteExt;
use tokio::{net::UnixStream, task::JoinHandle};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tokio_util::{
    io::{CopyToBytes, SinkWriter},
    sync::{PollSendError, PollSender},
};
use tonic::{async_trait, transport::Channel, Code, Status};
use tracing::instrument;

/// Connects to a (remote) tvix-store BlobService over gRPC.
#[derive(Clone)]
pub struct GRPCBlobService {
    /// The internal reference to a gRPC client.
    /// Cloning it is cheap, and it internally handles concurrent requests.
    grpc_client: proto::blob_service_client::BlobServiceClient<Channel>,
}

impl GRPCBlobService {
    /// construct a [GRPCBlobService] from a [proto::blob_service_client::BlobServiceClient].
    /// panics if called outside the context of a tokio runtime.
    pub fn from_client(
        grpc_client: proto::blob_service_client::BlobServiceClient<Channel>,
    ) -> Self {
        Self { grpc_client }
    }
}

#[async_trait]
impl BlobService for GRPCBlobService {
    /// Constructs a [GRPCBlobService] from the passed [url::Url]:
    /// - scheme has to match `grpc+*://`.
    ///   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
    /// - In the case of unix sockets, there must be a path, but may not be a host.
    /// - In the case of non-unix sockets, there must be a host, but no path.
    fn from_url(url: &url::Url) -> Result<Self, crate::Error> {
        // Start checking for the scheme to start with grpc+.
        match url.scheme().strip_prefix("grpc+") {
            None => Err(crate::Error::StorageError("invalid scheme".to_string())),
            Some(rest) => {
                if rest == "unix" {
                    if url.host_str().is_some() {
                        return Err(crate::Error::StorageError(
                            "host may not be set".to_string(),
                        ));
                    }
                    let path = url.path().to_string();
                    let channel = tonic::transport::Endpoint::try_from("http://[::]:50051") // doesn't matter
                        .unwrap()
                        .connect_with_connector_lazy(tower::service_fn(
                            move |_: tonic::transport::Uri| UnixStream::connect(path.clone()),
                        ));
                    let grpc_client = proto::blob_service_client::BlobServiceClient::new(channel);
                    Ok(Self::from_client(grpc_client))
                } else {
                    // ensure path is empty, not supported with gRPC.
                    if !url.path().is_empty() {
                        return Err(crate::Error::StorageError(
                            "path may not be set".to_string(),
                        ));
                    }

                    // clone the uri, and drop the grpc+ from the scheme.
                    // Recreate a new uri with the `grpc+` prefix dropped from the scheme.
                    // We can't use `url.set_scheme(rest)`, as it disallows
                    // setting something http(s) that previously wasn't.
                    let url = {
                        let url_str = url.to_string();
                        let s_stripped = url_str.strip_prefix("grpc+").unwrap();
                        url::Url::parse(s_stripped).unwrap()
                    };
                    let channel = tonic::transport::Endpoint::try_from(url.to_string())
                        .unwrap()
                        .connect_lazy();

                    let grpc_client = proto::blob_service_client::BlobServiceClient::new(channel);
                    Ok(Self::from_client(grpc_client))
                }
            }
        }
    }

    #[instrument(skip(self, digest), fields(blob.digest=%digest))]
    async fn has(&self, digest: &B3Digest) -> Result<bool, crate::Error> {
        let mut grpc_client = self.grpc_client.clone();
        let resp = grpc_client
            .stat(proto::StatBlobRequest {
                digest: digest.clone().into(),
                ..Default::default()
            })
            .await;

        match resp {
            Ok(_blob_meta) => Ok(true),
            Err(e) if e.code() == Code::NotFound => Ok(false),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    // On success, this returns a Ok(Some(io::Read)), which can be used to read
    // the contents of the Blob, identified by the digest.
    async fn open_read(
        &self,
        digest: &B3Digest,
    ) -> Result<Option<Box<dyn BlobReader>>, crate::Error> {
        // Get a new handle to the gRPC client, and copy the digest.
        let mut grpc_client = self.grpc_client.clone();

        // Get a stream of [proto::BlobChunk], or return an error if the blob
        // doesn't exist.
        let resp = grpc_client
            .read(proto::ReadBlobRequest {
                digest: digest.clone().into(),
            })
            .await;

        // This runs the task to completion, which on success will return a stream.
        // On reading from it, we receive individual [proto::BlobChunk], so we
        // massage this to a stream of bytes,
        // then create an [AsyncRead], which we'll turn into a [io::Read],
        // that's returned from the function.
        match resp {
            Ok(stream) => {
                // map the stream of proto::BlobChunk to bytes.
                let data_stream = stream.into_inner().map(|x| {
                    x.map(|x| VecDeque::from(x.data.to_vec()))
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
                });

                // Use StreamReader::new to convert to an AsyncRead.
                let data_reader = tokio_util::io::StreamReader::new(data_stream);

                Ok(Some(Box::new(NaiveSeeker::new(data_reader))))
            }
            Err(e) if e.code() == Code::NotFound => Ok(None),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    /// Returns a BlobWriter, that'll internally wrap each write in a
    // [proto::BlobChunk], which is send to the gRPC server.
    async fn open_write(&self) -> Box<dyn BlobWriter> {
        let mut grpc_client = self.grpc_client.clone();

        // set up an mpsc channel passing around Bytes.
        let (tx, rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(10);

        // bytes arriving on the RX side are wrapped inside a
        // [proto::BlobChunk], and a [ReceiverStream] is constructed.
        let blobchunk_stream = ReceiverStream::new(rx).map(|x| proto::BlobChunk { data: x });

        // That receiver stream is used as a stream in the gRPC BlobService.put rpc call.
        let task: JoinHandle<Result<_, Status>> =
            tokio::spawn(async move { Ok(grpc_client.put(blobchunk_stream).await?.into_inner()) });

        // The tx part of the channel is converted to a sink of byte chunks.

        // We need to make this a function pointer, not a closure.
        fn convert_error(_: PollSendError<bytes::Bytes>) -> io::Error {
            io::Error::from(io::ErrorKind::BrokenPipe)
        }

        let sink = PollSender::new(tx)
            .sink_map_err(convert_error as fn(PollSendError<bytes::Bytes>) -> io::Error);
        // We need to explicitly cast here, otherwise rustc does error with "expected fn pointer, found fn item"

        // … which is turned into an [tokio::io::AsyncWrite].
        let writer = SinkWriter::new(CopyToBytes::new(sink));

        Box::new(GRPCBlobWriter {
            task_and_writer: Some((task, writer)),
            digest: None,
        })
    }
}

pub struct GRPCBlobWriter<W: tokio::io::AsyncWrite> {
    /// The task containing the put request, and the inner writer, if we're still writing.
    task_and_writer: Option<(JoinHandle<Result<proto::PutBlobResponse, Status>>, W)>,

    /// The digest that has been returned, if we successfully closed.
    digest: Option<B3Digest>,
}

#[async_trait]
impl<W: tokio::io::AsyncWrite + Send + Sync + Unpin + 'static> BlobWriter for GRPCBlobWriter<W> {
    async fn close(&mut self) -> Result<B3Digest, crate::Error> {
        if self.task_and_writer.is_none() {
            // if we're already closed, return the b3 digest, which must exist.
            // If it doesn't, we already closed and failed once, and didn't handle the error.
            match &self.digest {
                Some(digest) => Ok(digest.clone()),
                None => Err(crate::Error::StorageError(
                    "previously closed with error".to_string(),
                )),
            }
        } else {
            let (task, mut writer) = self.task_and_writer.take().unwrap();

            // invoke shutdown, so the inner writer closes its internal tx side of
            // the channel.
            writer
                .shutdown()
                .map_err(|e| crate::Error::StorageError(e.to_string()))
                .await?;

            // block on the RPC call to return.
            // This ensures all chunks are sent out, and have been received by the
            // backend.

            match task.await? {
                Ok(resp) => {
                    // return the digest from the response, and store it in self.digest for subsequent closes.
                    let digest: B3Digest = resp.digest.try_into().map_err(|_| {
                        crate::Error::StorageError(
                            "invalid root digest length in response".to_string(),
                        )
                    })?;
                    self.digest = Some(digest.clone());
                    Ok(digest)
                }
                Err(e) => Err(crate::Error::StorageError(e.to_string())),
            }
        }
    }
}

impl<W: tokio::io::AsyncWrite + Unpin> tokio::io::AsyncWrite for GRPCBlobWriter<W> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, io::Error>> {
        match &mut self.task_and_writer {
            None => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "already closed",
            ))),
            Some((_, ref mut writer)) => {
                let pinned_writer = pin!(writer);
                pinned_writer.poll_write(cx, buf)
            }
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        match &mut self.task_and_writer {
            None => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "already closed",
            ))),
            Some((_, ref mut writer)) => {
                let pinned_writer = pin!(writer);
                pinned_writer.poll_flush(cx)
            }
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        // TODO(raitobezarius): this might not be a graceful shutdown of the
        // channel inside the gRPC connection.
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use tempfile::TempDir;
    use tokio::net::UnixListener;
    use tokio::time;
    use tokio_stream::wrappers::UnixListenerStream;

    use crate::blobservice::MemoryBlobService;
    use crate::proto::GRPCBlobServiceWrapper;
    use crate::tests::fixtures;

    use super::BlobService;
    use super::GRPCBlobService;

    /// This uses the wrong scheme
    #[test]
    fn test_invalid_scheme() {
        let url = url::Url::parse("http://foo.example/test").expect("must parse");

        assert!(GRPCBlobService::from_url(&url).is_err());
    }

    /// This uses the correct scheme for a unix socket.
    /// The fact that /path/to/somewhere doesn't exist yet is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_valid_unix_path() {
        let url = url::Url::parse("grpc+unix:///path/to/somewhere").expect("must parse");

        assert!(GRPCBlobService::from_url(&url).is_ok());
    }

    /// This uses the correct scheme for a unix socket,
    /// but sets a host, which is unsupported.
    #[tokio::test]
    async fn test_invalid_unix_path_with_domain() {
        let url =
            url::Url::parse("grpc+unix://host.example/path/to/somewhere").expect("must parse");

        assert!(GRPCBlobService::from_url(&url).is_err());
    }

    /// This uses the correct scheme for a HTTP server.
    /// The fact that nothing is listening there is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_valid_http() {
        let url = url::Url::parse("grpc+http://localhost").expect("must parse");

        assert!(GRPCBlobService::from_url(&url).is_ok());
    }

    /// This uses the correct scheme for a HTTPS server.
    /// The fact that nothing is listening there is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_valid_https() {
        let url = url::Url::parse("grpc+https://localhost").expect("must parse");

        assert!(GRPCBlobService::from_url(&url).is_ok());
    }

    /// This uses the correct scheme, but also specifies
    /// an additional path, which is not supported for gRPC.
    /// The fact that nothing is listening there is no problem, because we connect lazily.
    #[tokio::test]
    async fn test_invalid_http_with_path() {
        let url = url::Url::parse("grpc+https://localhost/some-path").expect("must parse");

        assert!(GRPCBlobService::from_url(&url).is_err());
    }

    /// This uses the correct scheme for a unix socket, and provides a server on the other side.
    /// This is not a tokio::test, because spawn two separate tokio runtimes and
    // want to have explicit control.
    #[test]
    fn test_valid_unix_path_ping_pong() {
        let tmpdir = TempDir::new().unwrap();
        let path = tmpdir.path().join("daemon");

        let path_clone = path.clone();

        // Spin up a server, in a thread far away, which spawns its own tokio runtime,
        // and blocks on the task.
        thread::spawn(move || {
            // Create the runtime
            let rt = tokio::runtime::Runtime::new().unwrap();

            let task = rt.spawn(async {
                let uds = UnixListener::bind(path_clone).unwrap();
                let uds_stream = UnixListenerStream::new(uds);

                // spin up a new server
                let mut server = tonic::transport::Server::builder();
                let router =
                    server.add_service(crate::proto::blob_service_server::BlobServiceServer::new(
                        GRPCBlobServiceWrapper::from(
                            Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>
                        ),
                    ));
                router.serve_with_incoming(uds_stream).await
            });

            rt.block_on(task).unwrap().unwrap();
        });

        // Now create another tokio runtime which we'll use in the main test code.
        let rt = tokio::runtime::Runtime::new().unwrap();

        let task = rt.spawn(async move {
            // wait for the socket to be created
            {
                let mut socket_created = false;
                // TODO: exponential backoff urgently
                for _try in 1..20 {
                    if path.exists() {
                        socket_created = true;
                        break;
                    }
                    tokio::time::sleep(time::Duration::from_millis(20)).await;
                }

                assert!(
                    socket_created,
                    "expected socket path to eventually get created, but never happened"
                );
            }

            // prepare a client
            let client = {
                let mut url =
                    url::Url::parse("grpc+unix:///path/to/somewhere").expect("must parse");
                url.set_path(path.to_str().unwrap());
                GRPCBlobService::from_url(&url).expect("must succeed")
            };

            let has = client
                .has(&fixtures::BLOB_A_DIGEST)
                .await
                .expect("must not be err");

            assert!(!has);
        });
        rt.block_on(task).unwrap()
    }
}
