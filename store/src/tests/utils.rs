use crate::{
    blobservice::{BlobService, MemoryBlobService},
    chunkservice::{ChunkService, MemoryChunkService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    pathinfoservice::{MemoryPathInfoService, PathInfoService},
};

pub fn gen_blob_service() -> impl BlobService + Send + Sync + Clone + 'static {
    MemoryBlobService::default()
}

pub fn gen_chunk_service() -> impl ChunkService + Clone {
    MemoryChunkService::default()
}

pub fn gen_directory_service() -> impl DirectoryService + Send + Sync + Clone + 'static {
    MemoryDirectoryService::default()
}

pub fn gen_pathinfo_service() -> impl PathInfoService {
    MemoryPathInfoService::default()
}
