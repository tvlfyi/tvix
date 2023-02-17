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

        // TODO: for now, we collect all Chunks into a large Vec<u8>, and then
        // pass it to a (content-defined) Chunker.
        // This is because the fastcdc crate currently operates on byte slices,
        // not on something implementing [std::io::Read].
        // (see https://github.com/nlfiedler/fastcdc-rs/issues/17)

        let mut blob_contents: Vec<u8> = Vec::new();

        while let Some(mut blob_chunk) = req_inner.message().await? {
            blob_contents.append(&mut blob_chunk.data);
        }

        // initialize a new chunker
        // TODO: play with chunking sizes
        let chunker = fastcdc::v2020::FastCDC::new(
            &blob_contents,
            64 * 1024 / 4, // min
            64 * 1024,     // avg
            64 * 1024 * 4, // max
        );

        // initialize blake3 hashers. chunk_hasher is used and reset for each
        // chunk, blob_hasher calculates the hash of the whole blob.
        let mut chunk_hasher = blake3::Hasher::new();
        let mut blob_hasher = blake3::Hasher::new();

        // start a BlobMeta, which we'll fill while looping over the chunks
        let mut blob_meta = super::BlobMeta::default();

        // loop over all the chunks
        for chunk in chunker {
            // extract the data itself
            let chunk_data: Vec<u8> =
                blob_contents[chunk.offset..chunk.offset + chunk.length].to_vec();

            // calculate the digest of that chunk
            chunk_hasher.update(&chunk_data);
            let chunk_digest = chunk_hasher.finalize();
            chunk_hasher.reset();

            // also update blob_hasher
            blob_hasher.update(&chunk_data);

            // check if chunk is already in db, and if not, insert.
            match self.chunk_service.has(chunk_digest.as_bytes()) {
                Err(e) => {
                    return Err(Error::StorageError(format!(
                        "unable to check if chunk {} exists: {}",
                        BASE64.encode(chunk_digest.as_bytes()),
                        e
                    ))
                    .into());
                }
                Ok(has_chunk) => {
                    if !has_chunk {
                        if let Err(e) = self.chunk_service.put(chunk_data.to_vec()) {
                            return Err(Error::StorageError(format!(
                                "unable to store chunk {}: {}",
                                BASE64.encode(chunk_digest.as_bytes()),
                                e
                            ))
                            .into());
                        }
                    }
                }
            }

            // add chunk to blobmeta
            blob_meta.chunks.push(super::blob_meta::ChunkMeta {
                digest: chunk_digest.as_bytes().to_vec(),
                size: chunk.length as u32,
            });
        }

        // done reading data, finalize blob_hasher and insert blobmeta.
        let blob_digest = blob_hasher.finalize();

        // TODO: don't store if we already have it (potentially with different chunking)
        match self.blob_service.put(blob_digest.as_bytes(), blob_meta) {
            Ok(()) => Ok(Response::new(super::PutBlobResponse {
                digest: blob_digest.as_bytes().to_vec(),
            })),
            Err(e) => Err(e.into()),
        }
    }
}
