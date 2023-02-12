use std::path::PathBuf;

use data_encoding::BASE64;
use prost::Message;
use tracing::instrument;

use crate::{proto, Error};

use super::BlobService;

#[derive(Clone)]
pub struct SledBlobService {
    db: sled::Db,
}

impl SledBlobService {
    pub fn new(p: PathBuf) -> Result<Self, sled::Error> {
        let config = sled::Config::default().use_compression(true).path(p);
        let db = config.open()?;

        Ok(Self { db })
    }
}

impl BlobService for SledBlobService {
    #[instrument(name = "SledBlobService::stat", skip(self, req), fields(blob.digest=BASE64.encode(&req.digest)))]
    fn stat(&self, req: &proto::StatBlobRequest) -> Result<Option<proto::BlobMeta>, Error> {
        if req.include_bao {
            todo!("not implemented yet")
        }

        // if include_chunks is also false, the user only wants to know if the
        // blob is present at all.
        if !req.include_chunks {
            match self.db.contains_key(&req.digest) {
                Ok(false) => Ok(None),
                Ok(true) => Ok(Some(proto::BlobMeta::default())),
                Err(e) => Err(Error::StorageError(e.to_string())),
            }
        } else {
            match self.db.get(&req.digest) {
                Ok(None) => Ok(None),
                Ok(Some(data)) => match proto::BlobMeta::decode(&*data) {
                    Ok(blob_meta) => Ok(Some(blob_meta)),
                    Err(e) => Err(Error::StorageError(format!(
                        "unable to parse blobmeta message for blob {}: {}",
                        BASE64.encode(&req.digest),
                        e
                    ))),
                },
                Err(e) => Err(Error::StorageError(e.to_string())),
            }
        }
    }

    #[instrument(name = "SledBlobService::put", skip(self, blob_meta, blob_digest), fields(blob.digest = BASE64.encode(blob_digest)))]
    fn put(&self, blob_digest: &[u8], blob_meta: proto::BlobMeta) -> Result<(), Error> {
        let result = self.db.insert(blob_digest, blob_meta.encode_to_vec());
        if let Err(e) = result {
            return Err(Error::StorageError(e.to_string()));
        }
        Ok(())
        // TODO: make sure all callers make sure the chunks exist.
        // TODO: where should we calculate the bao?
    }
}
