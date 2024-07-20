use axum::http::StatusCode;
use axum::routing::{head, put};
use axum::{routing::get, Router};
use lru::LruCache;
use parking_lot::RwLock;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tvix_castore::blobservice::BlobService;
use tvix_castore::directoryservice::DirectoryService;
use tvix_castore::proto::node::Node;
use tvix_store::pathinfoservice::PathInfoService;

mod nar;
mod narinfo;

/// The capacity of the lookup table from NarHash to [Node].
/// Should be bigger than the number of concurrent NAR upload.
/// Cannot be [NonZeroUsize] here due to rust-analyzer going bananas.
/// SAFETY: 1000 != 0
const ROOT_NODES_CACHE_CAPACITY: usize = 1000;

#[derive(Clone)]
pub struct AppState {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,

    /// Lookup table from NarHash to [Node], necessary to populate the root_node
    /// field of the PathInfo when processing the narinfo upload.
    root_nodes: Arc<RwLock<LruCache<[u8; 32], Node>>>,
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
            root_nodes: Arc::new(RwLock::new(LruCache::new({
                // SAFETY: 1000 != 0
                unsafe { NonZeroUsize::new_unchecked(ROOT_NODES_CACHE_CAPACITY) }
            }))),
        }
    }
}

pub fn gen_router(priority: u64) -> Router<AppState> {
    Router::new()
        .route("/", get(root))
        // FUTUREWORK: respond for NARs that we still have in root_nodes (at least HEAD)
        // This avoids some unnecessary NAR uploading from multiple concurrent clients, and is cheap.
        .route("/nar/:nar_str", get(four_o_four))
        .route("/nar/:nar_str", head(four_o_four))
        .route("/nar/:nar_str", put(nar::put))
        .route("/nar/tvix-castore/:root_node_enc", get(nar::get))
        .route("/:narinfo_str", get(narinfo::get))
        .route("/:narinfo_str", head(narinfo::head))
        .route("/:narinfo_str", put(narinfo::put))
        .route("/nix-cache-info", get(move || nix_cache_info(priority)))
}

async fn root() -> &'static str {
    "Hello from nar-bridge"
}

async fn four_o_four() -> Result<(), StatusCode> {
    Err(StatusCode::NOT_FOUND)
}

async fn nix_cache_info(priority: u64) -> String {
    format!(
        "StoreDir: /nix/store\nWantMassQuery: 1\nPriority: {}\n",
        priority
    )
}
