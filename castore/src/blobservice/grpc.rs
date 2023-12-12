use super::{naive_seeker::NaiveSeeker, BlobReader, BlobService, BlobWriter};
use crate::{proto, B3Digest};
use futures::sink::SinkExt;
use std::{
    collections::VecDeque,
    io::{self},
    pin::pin,
    task::Poll,
};
use tokio::io::AsyncWriteExt;
use tokio::task::JoinHandle;
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
    #[instrument(skip(self, digest), fields(blob.digest=%digest))]
    async fn has(&self, digest: &B3Digest) -> io::Result<bool> {
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
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
        }
    }

    // On success, this returns a Ok(Some(io::Read)), which can be used to read
    // the contents of the Blob, identified by the digest.
    async fn open_read(&self, digest: &B3Digest) -> io::Result<Option<Box<dyn BlobReader>>> {
        // Get a stream of [proto::BlobChunk], or return an error if the blob
        // doesn't exist.
        let resp = self
            .grpc_client
            .clone()
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
            Err(e) => Err(io::Error::new(io::ErrorKind::Other, e)),
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
    async fn close(&mut self) -> io::Result<B3Digest> {
        if self.task_and_writer.is_none() {
            // if we're already closed, return the b3 digest, which must exist.
            // If it doesn't, we already closed and failed once, and didn't handle the error.
            match &self.digest {
                Some(digest) => Ok(digest.clone()),
                None => Err(io::Error::new(io::ErrorKind::BrokenPipe, "already closed")),
            }
        } else {
            let (task, mut writer) = self.task_and_writer.take().unwrap();

            // invoke shutdown, so the inner writer closes its internal tx side of
            // the channel.
            writer.shutdown().await?;

            // block on the RPC call to return.
            // This ensures all chunks are sent out, and have been received by the
            // backend.

            match task.await? {
                Ok(resp) => {
                    // return the digest from the response, and store it in self.digest for subsequent closes.
                    let digest_len = resp.digest.len();
                    let digest: B3Digest = resp.digest.try_into().map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::Other,
                            format!("invalid root digest length {} in response", digest_len),
                        )
                    })?;
                    self.digest = Some(digest.clone());
                    Ok(digest)
                }
                Err(e) => Err(io::Error::new(io::ErrorKind::Other, e.to_string())),
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
    use std::time::Duration;

    use tempfile::TempDir;
    use tokio::net::UnixListener;
    use tokio_retry::strategy::ExponentialBackoff;
    use tokio_retry::Retry;
    use tokio_stream::wrappers::UnixListenerStream;

    use crate::blobservice::MemoryBlobService;
    use crate::fixtures;
    use crate::proto::blob_service_client::BlobServiceClient;
    use crate::proto::GRPCBlobServiceWrapper;

    use super::BlobService;
    use super::GRPCBlobService;

    /// This ensures connecting via gRPC works as expected.
    #[tokio::test]
    async fn test_valid_unix_path_ping_pong() {
        let tmpdir = TempDir::new().unwrap();
        let socket_path = tmpdir.path().join("daemon");

        let path_clone = socket_path.clone();

        // Spin up a server
        tokio::spawn(async {
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

        // wait for the socket to be created
        Retry::spawn(
            ExponentialBackoff::from_millis(20).max_delay(Duration::from_secs(10)),
            || async {
                if socket_path.exists() {
                    Ok(())
                } else {
                    Err(())
                }
            },
        )
        .await
        .expect("failed to wait for socket");

        // prepare a client
        let grpc_client = {
            let url = url::Url::parse(&format!(
                "grpc+unix://{}?wait-connect=1",
                socket_path.display()
            ))
            .expect("must parse");
            let client = BlobServiceClient::new(
                crate::tonic::channel_from_url(&url)
                    .await
                    .expect("must succeed"),
            );

            GRPCBlobService::from_client(client)
        };

        let has = grpc_client
            .has(&fixtures::BLOB_A_DIGEST)
            .await
            .expect("must not be err");

        assert!(!has);
    }
}
