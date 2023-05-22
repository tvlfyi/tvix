use super::{BlobService, BlobWriter};
use crate::{proto, B3Digest};
use futures::sink::{SinkExt, SinkMapErr};
use std::{collections::VecDeque, io};
use tokio::task::JoinHandle;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use tokio_util::{
    io::{CopyToBytes, SinkWriter, SyncIoBridge},
    sync::{PollSendError, PollSender},
};
use tonic::{transport::Channel, Code, Status, Streaming};
use tracing::instrument;

/// Connects to a (remote) tvix-store BlobService over gRPC.
#[derive(Clone)]
pub struct GRPCBlobService {
    /// A handle into the active tokio runtime. Necessary to spawn tasks.
    tokio_handle: tokio::runtime::Handle,

    /// The internal reference to a gRPC client.
    /// Cloning it is cheap, and it internally handles concurrent requests.
    grpc_client: proto::blob_service_client::BlobServiceClient<Channel>,
}

// TODO: provide some contstructors

impl BlobService for GRPCBlobService {
    type BlobReader = Box<dyn io::Read + Send>;
    type BlobWriter = GRPCBlobWriter;

    #[instrument(skip(self, digest), fields(blob.digest=%digest))]
    fn has(&self, digest: &B3Digest) -> Result<bool, crate::Error> {
        // Get a new handle to the gRPC client, and copy the digest.
        let mut grpc_client = self.grpc_client.clone();
        let digest = digest.clone();

        let task: tokio::task::JoinHandle<Result<_, Status>> =
            self.tokio_handle.spawn(async move {
                Ok(grpc_client
                    .stat(proto::StatBlobRequest {
                        digest: digest.to_vec(),
                        ..Default::default()
                    })
                    .await?
                    .into_inner())
            });

        match self.tokio_handle.block_on(task)? {
            Ok(_blob_meta) => Ok(true),
            Err(e) if e.code() == Code::NotFound => Ok(false),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    // On success, this returns a Ok(Some(io::Read)), which can be used to read
    // the contents of the Blob, identified by the digest.
    fn open_read(&self, digest: &B3Digest) -> Result<Option<Self::BlobReader>, crate::Error> {
        // Get a new handle to the gRPC client, and copy the digest.
        let mut grpc_client = self.grpc_client.clone();
        let digest = digest.clone();

        // Construct the task that'll send out the request and return the stream
        // the gRPC client should use to send [proto::BlobChunk], or an error if
        // the blob doesn't exist.
        let task: tokio::task::JoinHandle<Result<Streaming<proto::BlobChunk>, Status>> =
            self.tokio_handle.spawn(async move {
                let stream = grpc_client
                    .read(proto::ReadBlobRequest {
                        digest: digest.to_vec(),
                    })
                    .await?
                    .into_inner();

                Ok(stream)
            });

        // This runs the task to completion, which on success will return a stream.
        // On reading from it, we receive individual [proto::BlobChunk], so we
        // massage this to a stream of bytes,
        // then create an [AsyncRead], which we'll turn into a [io::Read],
        // that's returned from the function.
        match self.tokio_handle.block_on(task)? {
            Ok(stream) => {
                // map the stream of proto::BlobChunk to bytes.
                let data_stream = stream.map(|x| {
                    x.map(|x| VecDeque::from(x.data))
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
                });

                // Use StreamReader::new to convert to an AsyncRead.
                let data_reader = tokio_util::io::StreamReader::new(data_stream);

                // Use SyncIoBridge to turn it into a sync Read.
                let sync_reader = tokio_util::io::SyncIoBridge::new(data_reader);
                Ok(Some(Box::new(sync_reader)))
            }
            Err(e) if e.code() == Code::NotFound => Ok(None),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    /// Returns a [Self::BlobWriter], that'll internally wrap each write in a
    // [proto::BlobChunk] and which is passed to the
    fn open_write(&self) -> Result<Self::BlobWriter, crate::Error> {
        let mut grpc_client = self.grpc_client.clone();

        // set up an mpsc channel passing around Bytes.
        let (tx, rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(10);

        // bytes arriving on the RX side are wrapped inside a
        // [proto::BlobChunk], and a [ReceiverStream] is constructed.
        let blobchunk_stream =
            ReceiverStream::new(rx).map(|x| proto::BlobChunk { data: x.to_vec() });

        // That receiver stream is used as a stream in the gRPC BlobService.put rpc call.
        let task: tokio::task::JoinHandle<Result<_, Status>> = self
            .tokio_handle
            .spawn(async move { Ok(grpc_client.put(blobchunk_stream).await?.into_inner()) });

        // The tx part of the channel is converted to a sink of byte chunks.

        // We need to make this a function pointer, not a closure.
        fn convert_error(_: PollSendError<bytes::Bytes>) -> io::Error {
            io::Error::from(io::ErrorKind::BrokenPipe)
        }

        let sink = PollSender::new(tx)
            .sink_map_err(convert_error as fn(PollSendError<bytes::Bytes>) -> io::Error);
        // We need to explicitly cast here, otherwise rustc does error with "expected fn pointer, found fn item"

        // … which is turned into an [tokio::io::AsyncWrite].
        let async_writer = SinkWriter::new(CopyToBytes::new(sink));
        // … which is then turned into a [io::Write].
        let writer = SyncIoBridge::new(async_writer);

        Ok(GRPCBlobWriter {
            tokio_handle: self.tokio_handle.clone(), // TODO: is the clone() ok here?
            task,
            inner_writer: writer,
        })
    }
}

type BridgedWriter = SyncIoBridge<
    SinkWriter<
        CopyToBytes<
            SinkMapErr<PollSender<bytes::Bytes>, fn(PollSendError<bytes::Bytes>) -> io::Error>,
        >,
    >,
>;

pub struct GRPCBlobWriter {
    /// A handle into the active tokio runtime. Necessary to block on the task
    /// containing the put request.
    tokio_handle: tokio::runtime::Handle,

    /// The task containing the put request.
    task: JoinHandle<Result<proto::PutBlobResponse, Status>>,

    /// The inner Writer.
    inner_writer: BridgedWriter,
}

impl BlobWriter for GRPCBlobWriter {
    fn close(mut self) -> Result<B3Digest, crate::Error> {
        // invoke shutdown, so the inner writer closes its internal tx side of
        // the channel.
        self.inner_writer
            .shutdown()
            .map_err(|e| crate::Error::StorageError(e.to_string()))?;

        // block on the RPC call to return.
        // This ensures all chunks are sent out, and have been received by the
        // backend.
        match self.tokio_handle.block_on(self.task)? {
            Ok(resp) => {
                // return the digest from the response.
                B3Digest::from_vec(resp.digest).map_err(|_| {
                    crate::Error::StorageError("invalid root digest length in response".to_string())
                })
            }
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }
}

impl io::Write for GRPCBlobWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner_writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner_writer.flush()
    }
}
