//! A crate containing constructors to provide instances of a BlobService and
//! DirectoryService.
//! Only used for testing purposes, but across crates.
//! Should be removed once we have a better concept of a "Service registry".

use std::sync::Arc;

use crate::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
};

pub fn gen_blob_service() -> Arc<dyn BlobService> {
    Arc::new(MemoryBlobService::default())
}

pub fn gen_directory_service() -> Arc<dyn DirectoryService> {
    Arc::new(MemoryDirectoryService::default())
}
