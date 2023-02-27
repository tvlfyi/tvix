use crate::{blobservice::BlobService, chunkservice::ChunkService, Error};
use data_encoding::BASE64;
use std::io::{BufWriter, Write};
use tokio::{sync::mpsc::channel, task};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::{debug, error, instrument, warn};

pub struct GRPCBlobServiceWrapper<BS: BlobService, CS: ChunkService> {
    blob_service: BS,
    chunk_service: CS,
}

impl<BS: BlobService, CS: ChunkService> GRPCBlobServiceWrapper<BS, CS> {
    pub fn new(blob_service: BS, chunk_service: CS) -> Self {
        Self {
            blob_service,
            chunk_service,
        }
    }

    // upload the chunk to the chunk service, and return its digest (or an error) when done.
    #[instrument(skip(chunk_service))]
    fn upload_chunk(chunk_service: CS, chunk_data: Vec<u8>) -> Result<Vec<u8>, Error> {
        let mut hasher = blake3::Hasher::new();
        if chunk_data.len() >= 128 * 1024 {
            hasher.update_rayon(&chunk_data);
        } else {
            hasher.update(&chunk_data);
        }
        let digest = hasher.finalize();

        if chunk_service.has(digest.as_bytes())? {
            debug!("already has chunk, skipping");
        }
        let digest_resp = chunk_service.put(chunk_data)?;

        assert_eq!(digest_resp, digest.as_bytes());

        Ok(digest.as_bytes().to_vec())
    }
}

#[async_trait]
impl<
        BS: BlobService + Send + Sync + Clone + 'static,
        CS: ChunkService + Send + Sync + Clone + 'static,
    > super::blob_service_server::BlobService for GRPCBlobServiceWrapper<BS, CS>
{
    type ReadStream = ReceiverStream<Result<super::BlobChunk, Status>>;

    #[instrument(skip(self))]
    async fn stat(
        &self,
        request: Request<super::StatBlobRequest>,
    ) -> Result<Response<super::BlobMeta>, Status> {
        let rq = request.into_inner();
        match self.blob_service.stat(&rq) {
            Ok(None) => Err(Status::not_found(format!(
                "blob {} not found",
                BASE64.encode(&rq.digest)
            ))),
            Ok(Some(blob_meta)) => Ok(Response::new(blob_meta)),
            Err(e) => Err(e.into()),
        }
    }

    #[instrument(skip(self))]
    async fn read(
        &self,
        request: Request<super::ReadBlobRequest>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = channel(5);

        // query the chunk service for more detailed blob info
        let stat_resp = self.blob_service.stat(&super::StatBlobRequest {
            digest: req.digest.to_vec(),
            include_chunks: true,
            ..Default::default()
        })?;

        match stat_resp {
            None => {
                // If the stat didn't return any blobmeta, the client might
                // still have asked for a single chunk to be read.
                // Check the chunkstore.
                if let Some(data) = self.chunk_service.get(&req.digest)? {
                    // We already know the hash matches, and contrary to
                    // iterating over a blobmeta, we can't know the size,
                    // so send the contents of that chunk over,
                    // as the first (and only) element of the stream.
                    task::spawn(async move {
                        let res = Ok(super::BlobChunk { data });
                        // send the result to the client. If the client already left, that's also fine.
                        if (tx.send(res).await).is_err() {
                            debug!("receiver dropped");
                        }
                    });
                } else {
                    return Err(Status::not_found(format!(
                        "blob {} not found",
                        BASE64.encode(&req.digest),
                    )));
                }
            }
            Some(blobmeta) => {
                let chunk_client = self.chunk_service.clone();

                // TODO: use BlobReader?
                // But then we might not be able to send compressed chunks as-is.
                // Might require implementing https://docs.rs/futures-core/latest/futures_core/stream/trait.Stream.html for it
                // first, so we can .next().await in here.

                task::spawn(async move {
                    for chunkmeta in blobmeta.chunks {
                        // request chunk.
                        // We don't need to validate the digest again, as
                        // that's required for all implementations of ChunkService.
                        let res = match chunk_client.get(&chunkmeta.digest) {
                            Err(e) => Err(e.into()),
                            // TODO: make this a separate error type
                            Ok(None) => Err(Error::StorageError(format!(
                                "consistency error: chunk {} for blob {} not found",
                                BASE64.encode(&chunkmeta.digest),
                                BASE64.encode(&req.digest),
                            ))
                            .into()),
                            Ok(Some(data)) => {
                                // We already know the hash matches, but also
                                // check the size matches what chunkmeta said.
                                if data.len() as u32 != chunkmeta.size {
                                    Err(Error::StorageError(format!(
                                        "consistency error: chunk {} for blob {} has wrong size, expected {}, got {}",
                                        BASE64.encode(&chunkmeta.digest),
                                        BASE64.encode(&req.digest),
                                        chunkmeta.size,
                                        data.len(),
                                    )).into())
                                } else {
                                    // send out the current chunk
                                    // TODO: we might want to break this up further if too big?
                                    Ok(super::BlobChunk { data })
                                }
                            }
                        };
                        // send the result to the client
                        if (tx.send(res).await).is_err() {
                            debug!("receiver dropped");
                            break;
                        }
                    }
                });
            }
        }

        let receiver_stream = ReceiverStream::new(rx);
        Ok(Response::new(receiver_stream))
    }

    #[instrument(skip(self))]
    async fn put(
        &self,
        request: Request<Streaming<super::BlobChunk>>,
    ) -> Result<Response<super::PutBlobResponse>, Status> {
        let mut req_inner = request.into_inner();

        // instantiate a [BlobWriter] to write all data received with a client,
        // but wrap it in a pretty large (1MiB) [BufWriter] to prevent
        // excessive useless chunk attempts.
        let mut blob_writer = crate::BlobWriter::new(&self.chunk_service);
        {
            let mut blob_writer_buffered = BufWriter::with_capacity(1024 * 1024, &mut blob_writer);

            // receive data from the client, and write them all to the blob_writer.
            while let Some(blob_chunk) = req_inner.message().await? {
                if let Err(e) = blob_writer_buffered.write_all(&blob_chunk.data) {
                    error!(e=%e,"unable to write blob data");
                    return Err(Status::internal("unable to write blob data"));
                }
            }
            blob_writer_buffered.flush()?;
        }

        // run finalize
        let (blob_digest, blob_meta) = blob_writer
            .finalize()
            .map_err(|_| Status::internal("unable to finalize blob"))?;

        // check if we have the received blob in the [BlobService] already.
        let resp = self.blob_service.stat(&super::StatBlobRequest {
            digest: blob_digest.to_vec(),
            ..Default::default()
        })?;

        // if not, store.
        if resp.is_none() {
            self.blob_service.put(&blob_digest, blob_meta)?;
        }

        // return to client.
        Ok(Response::new(super::PutBlobResponse {
            digest: blob_digest,
        }))
    }
}
