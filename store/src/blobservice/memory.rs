use data_encoding::BASE64;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tracing::instrument;

use crate::{proto, Error};

use super::BlobService;

#[derive(Clone, Default)]
pub struct MemoryBlobService {
    db: Arc<RwLock<HashMap<Vec<u8>, proto::BlobMeta>>>,
}

impl BlobService for MemoryBlobService {
    #[instrument(skip(self, req), fields(blob.digest=BASE64.encode(&req.digest)))]
    fn stat(&self, req: &proto::StatBlobRequest) -> Result<Option<proto::BlobMeta>, Error> {
        if req.include_bao {
            todo!("not implemented yet")
        }

        let db = self.db.read().unwrap();
        // if include_chunks is also false, the user only wants to know if the
        // blob is present at all.
        if !req.include_chunks {
            Ok(if db.contains_key(&req.digest) {
                Some(proto::BlobMeta::default())
            } else {
                None
            })
        } else {
            match db.get(&req.digest) {
                None => Ok(None),
                Some(blob_meta) => Ok(Some(blob_meta.clone())),
            }
        }
    }

    #[instrument(skip(self, blob_meta, blob_digest), fields(blob.digest = BASE64.encode(blob_digest)))]
    fn put(&self, blob_digest: &[u8], blob_meta: proto::BlobMeta) -> Result<(), Error> {
        let mut db = self.db.write().unwrap();

        db.insert(blob_digest.to_vec(), blob_meta);

        Ok(())
        // TODO: make sure all callers make sure the chunks exist.
        // TODO: where should we calculate the bao?
    }
}
