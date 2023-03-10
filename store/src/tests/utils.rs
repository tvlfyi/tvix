use crate::{
    blobservice::{BlobService, MemoryBlobService},
    chunkservice::{ChunkService, MemoryChunkService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    pathinfoservice::{MemoryPathInfoService, PathInfoService},
};

pub fn gen_blob_service() -> impl BlobService + Send + Sync + Clone + 'static {
    MemoryBlobService::new()
}

pub fn gen_chunk_service() -> impl ChunkService + Clone {
    MemoryChunkService::new()
}

pub fn gen_directory_service() -> impl DirectoryService + Send + Sync + Clone + 'static {
    MemoryDirectoryService::new()
}

pub fn gen_pathinfo_service() -> impl PathInfoService {
    MemoryPathInfoService::default()
}
