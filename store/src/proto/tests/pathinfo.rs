use crate::proto::{self, Node, PathInfo, ValidatePathInfoError};
use lazy_static::lazy_static;
use nix_compat::store_path::{ParseStorePathError, StorePath};
use test_case::test_case;

lazy_static! {
    static ref DUMMY_DIGEST: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
    static ref DUMMY_DIGEST_2: Vec<u8> = vec![
        0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
}

const DUMMY_NAME: &str = "00000000000000000000000000000000-dummy";

#[test_case(
    None,
    Err(ValidatePathInfoError::NoNodePresent()) ;
    "No node"
)]
#[test_case(
    Some(Node { node: None }),
    Err(ValidatePathInfoError::NoNodePresent());
    "No node 2"
)]
fn validate_no_node(
    t_node: Option<proto::Node>,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: t_node,
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    proto::DirectoryNode {
        name: DUMMY_NAME.to_string(),
        digest: DUMMY_DIGEST.to_vec(),
        size: 0,
    },
    Ok(StorePath::from_string(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    proto::DirectoryNode {
        name: DUMMY_NAME.to_string(),
        digest: vec![],
        size: 0,
    },
    Err(ValidatePathInfoError::InvalidDigestLen(0));
    "invalid digest length"
)]
#[test_case(
    proto::DirectoryNode {
        name: "invalid".to_string(),
        digest: DUMMY_DIGEST.to_vec(),
        size: 0,
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".to_string(),
        ParseStorePathError::InvalidName("".to_string())
    ));
    "invalid node name"
)]
fn validate_directory(
    t_directory_node: proto::DirectoryNode,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::Directory(t_directory_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    proto::FileNode {
        name: DUMMY_NAME.to_string(),
        digest: DUMMY_DIGEST.to_vec(),
        size: 0,
        executable: false,
    },
    Ok(StorePath::from_string(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    proto::FileNode {
        name: DUMMY_NAME.to_string(),
        digest: vec![],
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidDigestLen(0));
    "invalid digest length"
)]
#[test_case(
    proto::FileNode {
        name: "invalid".to_string(),
        digest: DUMMY_DIGEST.to_vec(),
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".to_string(),
        ParseStorePathError::InvalidName("".to_string())
    ));
    "invalid node name"
)]
fn validate_file(t_file_node: proto::FileNode, t_result: Result<StorePath, ValidatePathInfoError>) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::File(t_file_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    proto::SymlinkNode {
        name: DUMMY_NAME.to_string(),
        ..Default::default()
    },
    Ok(StorePath::from_string(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    proto::SymlinkNode {
        name: "invalid".to_string(),
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".to_string(),
        ParseStorePathError::InvalidName("".to_string())
    ));
    "invalid node name"
)]
fn validate_symlink(
    t_symlink_node: proto::SymlinkNode,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::Symlink(t_symlink_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test]
fn validate_references() {
    // create a PathInfo without narinfo field.
    let path_info = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::Directory(proto::DirectoryNode {
                name: DUMMY_NAME.to_string(),
                digest: DUMMY_DIGEST.to_vec(),
                size: 0,
            })),
        }),
        references: vec![DUMMY_DIGEST_2.to_vec()],
        narinfo: None,
    };
    assert!(path_info.validate().is_ok());

    // create a PathInfo with a narinfo field, but an inconsistent set of references
    let path_info_with_narinfo_missing_refs = PathInfo {
        narinfo: Some(proto::NarInfo {
            nar_size: 0,
            nar_sha256: DUMMY_DIGEST.to_vec(),
            signatures: vec![],
            reference_names: vec![],
        }),
        ..path_info.clone()
    };
    match path_info_with_narinfo_missing_refs
        .validate()
        .expect_err("must_fail")
    {
        ValidatePathInfoError::InconsistentNumberOfReferences(_, _) => {}
        _ => panic!("unexpected error"),
    };

    // create a pathinfo with the correct number of references, should suceed
    let path_info_with_narinfo = PathInfo {
        narinfo: Some(proto::NarInfo {
            nar_size: 0,
            nar_sha256: DUMMY_DIGEST.to_vec(),
            signatures: vec![],
            reference_names: vec![format!("/nix/store/{}", DUMMY_NAME)],
        }),
        ..path_info
    };
    assert!(path_info_with_narinfo.validate().is_ok());
}
