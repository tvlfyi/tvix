use crate::{proto, Error};
use std::io::Read;
use tracing::{debug, instrument};

use super::ChunkService;

/// uploads a chunk to a chunk service, and returns its digest (or an error) when done.
#[instrument(skip_all, err)]
pub fn upload_chunk<CS: ChunkService>(
    chunk_service: &CS,
    chunk_data: Vec<u8>,
) -> Result<[u8; 32], Error> {
    let mut hasher = blake3::Hasher::new();
    update_hasher(&mut hasher, &chunk_data);
    let digest = hasher.finalize();

    if chunk_service.has(digest.as_bytes())? {
        debug!("already has chunk, skipping");
    }
    let digest_resp = chunk_service.put(chunk_data)?;

    assert_eq!(&digest_resp, digest.as_bytes());

    Ok(digest.as_bytes().clone())
}

/// reads through a reader, writes chunks to a [ChunkService] and returns a
/// [proto::BlobMeta] pointing to all the chunks.
#[instrument(skip_all, err)]
pub fn read_all_and_chunk<CS: ChunkService, R: Read>(
    chunk_service: &CS,
    r: R,
) -> Result<(Vec<u8>, proto::BlobMeta), Error> {
    let mut blob_meta = proto::BlobMeta::default();

    // hash the file contents, upload chunks if not there yet
    let mut blob_hasher = blake3::Hasher::new();

    // TODO: play with chunking sizes
    let chunker_avg_size = 64 * 1024;
    let chunker_min_size = chunker_avg_size / 4;
    let chunker_max_size = chunker_avg_size * 4;

    let chunker =
        fastcdc::v2020::StreamCDC::new(r, chunker_min_size, chunker_avg_size, chunker_max_size);

    for chunking_result in chunker {
        let chunk = chunking_result.unwrap();
        // TODO: convert to error::UnableToRead

        let chunk_len = chunk.data.len() as u32;

        // update calculate blob hash
        update_hasher(&mut blob_hasher, &chunk.data);

        let chunk_digest = upload_chunk(chunk_service, chunk.data)?;

        blob_meta.chunks.push(proto::blob_meta::ChunkMeta {
            digest: chunk_digest.to_vec(),
            size: chunk_len,
        });
    }
    Ok((blob_hasher.finalize().as_bytes().to_vec(), blob_meta))
}

/// updates a given hasher with more data. Uses rayon if the data is
/// sufficiently big.
///
/// From the docs:
///
/// To get any performance benefit from multithreading, the input buffer needs
/// to be large. As a rule of thumb on x86_64, update_rayon is slower than
/// update for inputs under 128 KiB. That threshold varies quite a lot across
/// different processors, and it’s important to benchmark your specific use
/// case.
///
/// We didn't benchmark yet, so these numbers might need tweaking.
#[instrument(skip_all)]
pub fn update_hasher(hasher: &mut blake3::Hasher, data: &[u8]) {
    if data.len() > 128 * 1024 {
        hasher.update_rayon(data);
    } else {
        hasher.update(data);
    }
}
