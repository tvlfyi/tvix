use crate::{
    blobservice::{BlobService, SledBlobService},
    chunkservice::{ChunkService, SledChunkService},
    directoryservice::{DirectoryService, SledDirectoryService},
    pathinfoservice::{PathInfoService, SledPathInfoService},
};

pub fn gen_blob_service() -> impl BlobService + Send + Sync + Clone + 'static {
    SledBlobService::new_temporary().unwrap()
}

pub fn gen_chunk_service() -> impl ChunkService + Clone {
    SledChunkService::new_temporary().unwrap()
}

pub fn gen_directory_service() -> impl DirectoryService + Send + Sync + Clone + 'static {
    SledDirectoryService::new_temporary().unwrap()
}

pub fn gen_pathinfo_service() -> impl PathInfoService {
    SledPathInfoService::new_temporary().unwrap()
}
