use crate::{
    blobservice::{BlobService, BlobWriter},
    proto::sync_read_into_async_read::SyncReadIntoAsyncRead,
};
use data_encoding::BASE64;
use std::{collections::VecDeque, io, pin::Pin};
use tokio::task;
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::{instrument, warn};

pub struct GRPCBlobServiceWrapper<BS: BlobService> {
    blob_service: BS,
}

impl<BS: BlobService> From<BS> for GRPCBlobServiceWrapper<BS> {
    fn from(value: BS) -> Self {
        Self {
            blob_service: value,
        }
    }
}

#[async_trait]
impl<BS: BlobService + Send + Sync + Clone + 'static> super::blob_service_server::BlobService
    for GRPCBlobServiceWrapper<BS>
{
    // https://github.com/tokio-rs/tokio/issues/2723#issuecomment-1534723933
    type ReadStream =
        Pin<Box<dyn futures::Stream<Item = Result<super::BlobChunk, Status>> + Send + 'static>>;

    #[instrument(skip(self))]
    async fn stat(
        &self,
        request: Request<super::StatBlobRequest>,
    ) -> Result<Response<super::BlobMeta>, Status> {
        let rq = request.into_inner();
        let req_digest: [u8; 32] = rq
            .digest
            .clone()
            .try_into()
            .map_err(|_e| Status::invalid_argument("invalid digest length"))?;

        if rq.include_chunks || rq.include_bao {
            return Err(Status::internal("not implemented"));
        }

        match self.blob_service.has(&req_digest) {
            Ok(true) => Ok(Response::new(super::BlobMeta::default())),
            Ok(false) => Err(Status::not_found(format!(
                "blob {} not found",
                BASE64.encode(&req_digest)
            ))),
            Err(e) => Err(e.into()),
        }
    }

    #[instrument(skip(self))]
    async fn read(
        &self,
        request: Request<super::ReadBlobRequest>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        let rq = request.into_inner();

        let req_digest: [u8; 32] = rq
            .digest
            .clone()
            .try_into()
            .map_err(|_| Status::invalid_argument("invalid digest length"))?;

        match self.blob_service.open_read(&req_digest) {
            Ok(Some(reader)) => {
                let async_reader: SyncReadIntoAsyncRead<_, bytes::BytesMut> = reader.into();

                fn stream_mapper(
                    x: Result<bytes::Bytes, io::Error>,
                ) -> Result<super::BlobChunk, Status> {
                    match x {
                        Ok(bytes) => Ok(super::BlobChunk {
                            data: bytes.to_vec(),
                        }),
                        Err(e) => Err(Status::from(e)),
                    }
                }

                let chunks_stream = ReaderStream::new(async_reader).map(stream_mapper);
                Ok(Response::new(Box::pin(chunks_stream)))
            }
            Ok(None) => Err(Status::not_found(format!(
                "blob {} not found",
                BASE64.encode(&rq.digest)
            ))),
            Err(e) => Err(e.into()),
        }
    }

    #[instrument(skip(self))]
    async fn put(
        &self,
        request: Request<Streaming<super::BlobChunk>>,
    ) -> Result<Response<super::PutBlobResponse>, Status> {
        let req_inner = request.into_inner();

        let data_stream = req_inner.map(|x| {
            x.map(|x| VecDeque::from(x.data))
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))
        });

        let data_reader = tokio_util::io::StreamReader::new(data_stream);

        // prepare a writer, which we'll use in the blocking task below.
        let mut writer = self
            .blob_service
            .open_write()
            .map_err(|e| Status::internal(format!("unable to open for write: {}", e)))?;

        let result = task::spawn_blocking(move || -> Result<super::PutBlobResponse, Status> {
            // construct a sync reader to the data
            let mut reader = tokio_util::io::SyncIoBridge::new(data_reader);

            io::copy(&mut reader, &mut writer).map_err(|e| {
                warn!("error copying: {}", e);
                Status::internal("error copying")
            })?;

            let digest = writer
                .close()
                .map_err(|e| {
                    warn!("error closing stream: {}", e);
                    Status::internal("error closing stream")
                })?
                .to_vec();

            Ok(super::PutBlobResponse { digest })
        })
        .await
        .map_err(|_| Status::internal("failed to wait for task"))??;

        Ok(Response::new(result))
    }
}
