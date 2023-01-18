use tempfile::TempDir;
use tonic::Request;

use crate::proto::get_path_info_request::ByWhat::ByOutputHash;
use crate::proto::node::Node::Symlink;
use crate::proto::path_info_service_server::PathInfoService;
use crate::proto::PathInfo;
use crate::proto::{GetPathInfoRequest, Node, SymlinkNode};
use crate::sled_path_info_service::SledPathInfoService;

use lazy_static::lazy_static;

lazy_static! {
    static ref DUMMY_OUTPUT_HASH: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00
    ];
}

/// Trying to get a non-existent PathInfo should return a not found error.
#[tokio::test]
async fn not_found() -> anyhow::Result<()> {
    let service = SledPathInfoService::new(TempDir::new()?.path().to_path_buf())?;

    let resp = service
        .get(Request::new(GetPathInfoRequest {
            by_what: Some(ByOutputHash(DUMMY_OUTPUT_HASH.to_vec())),
        }))
        .await;

    let resp = resp.expect_err("must fail");
    assert_eq!(resp.code(), tonic::Code::NotFound);

    Ok(())
}

/// Put a PathInfo into the store, get it back.
#[tokio::test]
async fn put_get() -> anyhow::Result<()> {
    let service = SledPathInfoService::new(TempDir::new()?.path().to_path_buf())?;

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

    Ok(())
}
