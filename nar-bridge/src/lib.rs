use axum::routing::head;
use axum::{routing::get, Router};
use std::sync::Arc;
use tvix_castore::blobservice::BlobService;
use tvix_castore::directoryservice::DirectoryService;
use tvix_store::pathinfoservice::PathInfoService;

mod nar;
mod narinfo;

#[derive(Clone)]
pub struct AppState {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,
}

impl AppState {
    pub fn new(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
        path_info_service: Arc<dyn PathInfoService>,
    ) -> Self {
        Self {
            blob_service,
            directory_service,
            path_info_service,
        }
    }
}

pub fn gen_router(priority: u64) -> Router<AppState> {
    Router::new()
        .route("/", get(root))
        .route("/nar/tvix-castore/:root_node_enc", get(nar::get))
        .route("/:narinfo_str", get(narinfo::get))
        .route("/:narinfo_str", head(narinfo::head))
        .route("/nix-cache-info", get(move || nix_cache_info(priority)))
}

async fn root() -> &'static str {
    "Hello from nar-bridge"
}

async fn nix_cache_info(priority: u64) -> String {
    format!(
        "StoreDir: /nix/store\nWantMassQuery: 1\nPriority: {}\n",
        priority
    )
}
