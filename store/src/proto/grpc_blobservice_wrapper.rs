use crate::{blobservice::BlobService, chunkservice::ChunkService, Error};
use data_encoding::BASE64;
use tokio::{sync::mpsc::channel, task};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{async_trait, Request, Response, Status, Streaming};
use tracing::{debug, instrument};

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

        // initialize a blake3 hasher calculating the hash of the whole blob.
        let mut blob_hasher = blake3::Hasher::new();

        // start a BlobMeta, which we'll fill while looping over the chunks
        let mut blob_meta = super::BlobMeta::default();

        // is filled with bytes received from the client.
        let mut buf: Vec<u8> = vec![];

        // This reads data from the client, chunks it up using fastcdc,
        // uploads all chunks to the [ChunkService], and fills a
        // [super::BlobMeta] linking to these chunks.
        while let Some(blob_chunk) = req_inner.message().await? {
            // calculate blob hash, and use rayon if data is > 128KiB.
            if blob_chunk.data.len() > 128 * 1024 {
                blob_hasher.update_rayon(&blob_chunk.data);
            } else {
                blob_hasher.update(&blob_chunk.data);
            }

            // extend buf with the newly received data
            buf.append(&mut blob_chunk.data.clone());

            // TODO: play with chunking sizes
            let chunker_avg_size = 64 * 1024;
            let chunker_min_size = chunker_avg_size / 4;
            let chunker_max_size = chunker_avg_size * 4;

            // initialize a chunker with the current buffer
            let chunker = fastcdc::v2020::FastCDC::new(
                &buf,
                chunker_min_size,
                chunker_avg_size,
                chunker_max_size,
            );

            // ask the chunker for cutting points in the buffer.
            let mut start_pos = 0 as usize;
            buf = loop {
                // ask the chunker for the next cutting point.
                let (_fp, end_pos) = chunker.cut(start_pos, buf.len() - start_pos);

                // whenever the last cut point is pointing to the end of the buffer,
                // keep that chunk left in there.
                // We don't know if the chunker decided to cut here simply because it was
                // at the end of the buffer, or if it would also cut if there
                // were more data.
                //
                // Split off all previous chunks and keep this chunk data in the buffer.
                if end_pos == buf.len() {
                    break buf.split_off(start_pos);
                }

                // Upload that chunk to the chunk service and record it in BlobMeta.
                // TODO: make upload_chunk async and upload concurrently?
                let chunk_data = &buf[start_pos..end_pos];
                let chunk_digest =
                    Self::upload_chunk(self.chunk_service.clone(), chunk_data.to_vec())?;

                blob_meta.chunks.push(super::blob_meta::ChunkMeta {
                    digest: chunk_digest,
                    size: chunk_data.len() as u32,
                });

                // move start_pos over the processed chunk.
                start_pos = end_pos;
            }
        }

        // Also upload the last chunk (what's left in `buf`) to the chunk
        // service and record it in BlobMeta.
        let buf_len = buf.len() as u32;
        let chunk_digest = Self::upload_chunk(self.chunk_service.clone(), buf)?;

        blob_meta.chunks.push(super::blob_meta::ChunkMeta {
            digest: chunk_digest,
            size: buf_len,
        });

        let blob_digest = blob_hasher.finalize().as_bytes().to_vec();

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
