use std::path::Path;

use crate::{
    blobservice::{BlobService, SledBlobService},
    chunkservice::{ChunkService, SledChunkService},
    directoryservice::{DirectoryService, SledDirectoryService},
    pathinfoservice::{PathInfoService, SledPathInfoService},
};

pub fn gen_blob_service(p: &Path) -> impl BlobService + Send + Sync + Clone + 'static {
    SledBlobService::new(p.join("blobs")).unwrap()
}

pub fn gen_chunk_service(p: &Path) -> impl ChunkService + Clone {
    SledChunkService::new(p.join("chunks")).unwrap()
}

pub fn gen_directory_service(p: &Path) -> impl DirectoryService + Send + Sync + Clone + 'static {
    SledDirectoryService::new(p.join("directories")).unwrap()
}

pub fn gen_pathinfo_service(p: &Path) -> impl PathInfoService {
    SledPathInfoService::new(p.join("pathinfo")).unwrap()
}
