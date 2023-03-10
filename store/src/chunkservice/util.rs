use tracing::{debug, instrument};

use crate::Error;

use super::ChunkService;

/// uploads a chunk to a chunk service, and returns its digest (or an error) when done.
#[instrument(skip_all, err)]
pub fn upload_chunk<CS: ChunkService>(
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
