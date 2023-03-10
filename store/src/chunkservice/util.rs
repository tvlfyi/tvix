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
    update_hasher(&mut hasher, &chunk_data);
    let digest = hasher.finalize();

    if chunk_service.has(digest.as_bytes())? {
        debug!("already has chunk, skipping");
    }
    let digest_resp = chunk_service.put(chunk_data)?;

    assert_eq!(digest_resp, digest.as_bytes());

    Ok(digest.as_bytes().to_vec())
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
