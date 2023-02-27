use crate::nar::NonCachingNARCalculationService;
use crate::proto::get_path_info_request::ByWhat::ByOutputHash;
use crate::proto::node::Node::Symlink;
use crate::proto::path_info_service_server::PathInfoService as GRPCPathInfoService;
use crate::proto::GRPCPathInfoServiceWrapper;
use crate::proto::PathInfo;
use crate::proto::{GetPathInfoRequest, Node, SymlinkNode};
use crate::tests::utils::{
    gen_blob_service, gen_chunk_service, gen_directory_service, gen_pathinfo_service,
};
use lazy_static::lazy_static;
use std::path::Path;
use tempfile::TempDir;
use tonic::Request;

lazy_static! {
    static ref DUMMY_OUTPUT_HASH: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00
    ];
}

/// generates a GRPCPathInfoService out of blob, chunk, directory and pathinfo services.
///
/// We only interact with it via the PathInfo GRPC interface.
/// It uses the NonCachingNARCalculationService NARCalculationService to
/// calculate NARs.
fn gen_grpc_service(p: &Path) -> impl GRPCPathInfoService {
    GRPCPathInfoServiceWrapper::new(
        gen_pathinfo_service(p),
        NonCachingNARCalculationService::new(
            gen_blob_service(p),
            gen_chunk_service(p),
            gen_directory_service(p),
        ),
    )
}

/// Trying to get a non-existent PathInfo should return a not found error.
#[tokio::test]
async fn not_found() {
    let tmpdir = TempDir::new().unwrap();
    let service = gen_grpc_service(tmpdir.path());

    let resp = service
        .get(Request::new(GetPathInfoRequest {
            by_what: Some(ByOutputHash(DUMMY_OUTPUT_HASH.to_vec())),
        }))
        .await;

    let resp = resp.expect_err("must fail");
    assert_eq!(resp.code(), tonic::Code::NotFound);
}

/// Put a PathInfo into the store, get it back.
#[tokio::test]
async fn put_get() {
    let tmpdir = TempDir::new().unwrap();
    let service = gen_grpc_service(tmpdir.path());

    let path_info = PathInfo {
        node: Some(Node {
            node: Some(Symlink(SymlinkNode {
                name: "00000000000000000000000000000000-foo".to_string(),
                target: "doesntmatter".to_string(),
            })),
        }),
        ..Default::default()
    };

    let resp = service.put(Request::new(path_info.clone())).await;

    assert!(resp.is_ok());
    assert_eq!(resp.expect("must succeed").into_inner(), path_info);

    let resp = service
        .get(Request::new(GetPathInfoRequest {
            by_what: Some(ByOutputHash(DUMMY_OUTPUT_HASH.to_vec())),
        }))
        .await;

    assert!(resp.is_ok());
    assert_eq!(resp.expect("must succeed").into_inner(), path_info);
}
