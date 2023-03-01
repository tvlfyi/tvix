use crate::chunkservice::ChunkService;
use crate::{proto, Error};
use rayon::prelude::*;
use tracing::{debug, instrument};

pub struct BlobWriter<'a, CS: ChunkService> {
    chunk_service: &'a CS,

    blob_hasher: blake3::Hasher,

    blob_meta: proto::BlobMeta,

    // filled with data from previous writes that didn't end up producing a full chunk.
    buf: Vec<u8>,
}

// upload a chunk to the chunk service, and return its digest (or an error) when done.
#[instrument(skip_all)]
fn upload_chunk<CS: ChunkService>(
    chunk_service: &CS,
    chunk_data: Vec<u8>,
) -> Result<Vec<u8>, Error> {
    let mut hasher = blake3::Hasher::new();
    // TODO: benchmark this number and factor it out
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

impl<'a, CS: ChunkService> BlobWriter<'a, CS> {
    pub fn new(chunk_service: &'a CS) -> Self {
        Self {
            chunk_service,
            blob_hasher: blake3::Hasher::new(),
            blob_meta: proto::BlobMeta::default(),
            buf: vec![],
        }
    }

    // Return the digest of the blob, as well as the blobmeta containing info of all the chunks,
    // or an error if there's still bytes left to be flushed.
    // In case there was still data to be written a last unfinalized chunk,
    // it's written as well.
    #[instrument(skip(self))]
    pub fn finalize(&mut self) -> Result<(Vec<u8>, proto::BlobMeta), Error> {
        // If there's something still left in self.buf, upload this as the last
        // chunk to the chunk service and record it in BlobMeta.
        if !self.buf.is_empty() {
            // Also upload the last chunk (what's left in `self.buf`) to the chunk
            // service and record it in BlobMeta.
            let buf_len = self.buf.len() as u32;
            let chunk_digest = upload_chunk(self.chunk_service, self.buf.clone())?;

            self.blob_meta.chunks.push(proto::blob_meta::ChunkMeta {
                digest: chunk_digest,
                size: buf_len,
            });

            self.buf.clear();
        }
        return Ok((
            self.blob_hasher.finalize().as_bytes().to_vec(),
            self.blob_meta.clone(),
        ));
    }
}

/// This chunks up all data written using fastcdc, uploads all chunks to the
// [ChunkService], and fills a [proto::BlobMeta] linking to these chunks.
impl<CS: ChunkService + std::marker::Sync> std::io::Write for BlobWriter<'_, CS> {
    fn write(&mut self, input_buf: &[u8]) -> std::io::Result<usize> {
        // calculate input_buf.len(), we need to return that later.
        let input_buf_len = input_buf.len();

        // update calculate blob hash, and use rayon if data is > 128KiB.
        if input_buf.len() > 128 * 1024 {
            self.blob_hasher.update_rayon(input_buf);
        } else {
            self.blob_hasher.update(input_buf);
        }

        // prepend buf with existing data (from self.buf)
        let buf: Vec<u8> = {
            let mut b = Vec::new();
            b.append(&mut self.buf);
            b.append(&mut input_buf.to_vec());
            b
        };

        // TODO: play with chunking sizes
        let chunker_avg_size = 64 * 1024;
        let chunker_min_size = chunker_avg_size / 4;
        let chunker_max_size = chunker_avg_size * 4;

        // initialize a chunker with the buffer
        let chunker = fastcdc::v2020::FastCDC::new(
            &buf,
            chunker_min_size,
            chunker_avg_size,
            chunker_max_size,
        );

        // assemble a list of byte slices to be uploaded
        let mut chunk_slices: Vec<&[u8]> = Vec::new();

        // ask the chunker for cutting points in the buffer.
        let mut start_pos = 0_usize;
        let rest = loop {
            // ask the chunker for the next cutting point.
            let (_fp, end_pos) = chunker.cut(start_pos, buf.len() - start_pos);

            // whenever the last cut point is pointing to the end of the buffer,
            // return that from the loop.
            // We don't know if the chunker decided to cut here simply because it was
            // at the end of the buffer, or if it would also cut if there
            // were more data.
            //
            // Split off all previous chunks and keep this chunk data in the buffer.
            if end_pos == buf.len() {
                break &buf[start_pos..];
            }

            // if it's an intermediate chunk, add it to chunk_slices.
            // We'll later upload all of them in batch.
            chunk_slices.push(&buf[start_pos..end_pos]);

            // advance start_pos over the processed chunk.
            start_pos = end_pos;
        };

        // Upload all chunks to the chunk service and map them to a ChunkMeta
        let blob_meta_chunks: Vec<Result<proto::blob_meta::ChunkMeta, Error>> = chunk_slices
            .into_par_iter()
            .map(|chunk_slice| {
                let chunk_service = self.chunk_service.clone();
                let chunk_digest = upload_chunk(chunk_service.clone(), chunk_slice.to_vec())?;

                Ok(proto::blob_meta::ChunkMeta {
                    digest: chunk_digest,
                    size: chunk_slice.len() as u32,
                })
            })
            .collect();

        self.blob_meta.chunks = blob_meta_chunks
            .into_iter()
            .collect::<Result<Vec<proto::blob_meta::ChunkMeta>, Error>>()?;

        // update buf to point to the rest we didn't upload.
        self.buf = rest.to_vec();

        Ok(input_buf_len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
