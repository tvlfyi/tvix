use crate::pathinfoservice::PathInfo;
use crate::proto::{self, ValidatePathInfoError};
use crate::tests::fixtures::{DUMMY_PATH, DUMMY_PATH_DIGEST, DUMMY_PATH_STR};
use bytes::Bytes;
use lazy_static::lazy_static;
use nix_compat::store_path;
use rstest::rstest;
use tvix_castore::fixtures::DUMMY_DIGEST;
use tvix_castore::proto as castorepb;
use tvix_castore::{DirectoryError, ValidateNodeError};

lazy_static! {
    /// A valid PathInfo message
    /// The references in `narinfo.reference_names` aligns with what's in
    /// `references`.
    static ref PROTO_PATH_INFO : proto::PathInfo = proto::PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Directory(castorepb::DirectoryNode {
                name: DUMMY_PATH_STR.into(),
                digest: DUMMY_DIGEST.clone().into(),
                size: 0,
            })),
        }),
        references: vec![DUMMY_PATH_DIGEST.as_slice().into()],
        narinfo: Some(proto::NarInfo {
            nar_size: 0,
            nar_sha256: DUMMY_DIGEST.clone().into(),
            signatures: vec![],
            reference_names: vec![DUMMY_PATH_STR.to_string()],
            deriver: None,
            ca: Some(proto::nar_info::Ca { r#type: proto::nar_info::ca::Hash::NarSha256.into(), digest:  DUMMY_DIGEST.clone().into() })
        }),
    };
}

#[test]
fn convert_valid() {
    let path_info = PROTO_PATH_INFO.clone();
    PathInfo::try_from(path_info).expect("must succeed");
}

/// Create a PathInfo with a correct deriver field and ensure it succeeds.
#[test]
fn convert_valid_deriver() {
    let mut path_info = PROTO_PATH_INFO.clone();

    // add a valid deriver
    let narinfo = path_info.narinfo.as_mut().unwrap();
    narinfo.deriver = Some(crate::proto::StorePath {
        name: DUMMY_PATH.name().to_string(),
        digest: Bytes::from(DUMMY_PATH_DIGEST.as_slice()),
    });

    let path_info = PathInfo::try_from(path_info).expect("must succeed");
    assert_eq!(DUMMY_PATH.clone(), path_info.deriver.unwrap())
}

#[rstest]
#[case::no_node(None, ValidatePathInfoError::NoNodePresent)]
#[case::no_node_2(Some(castorepb::Node { node: None}), ValidatePathInfoError::InvalidRootNode(DirectoryError::NoNodeSet))]
fn convert_pathinfo_wrong_nodes(
    #[case] node: Option<castorepb::Node>,
    #[case] exp_err: ValidatePathInfoError,
) {
    // construct the PathInfo object
    let mut path_info = PROTO_PATH_INFO.clone();
    path_info.node = node;

    assert_eq!(
        exp_err,
        PathInfo::try_from(path_info).expect_err("must fail")
    );
}

/// Constructs a [proto::PathInfo] with root nodes that have wrong data in
/// various places, causing the conversion to [PathInfo] to fail.
#[rstest]
#[case::directory_invalid_digest_length(
    castorepb::node::Node::Directory(castorepb::DirectoryNode {
        name: DUMMY_PATH_STR.into(),
        digest: Bytes::new(),
        size: 0,
    }),
    ValidatePathInfoError::InvalidRootNode(DirectoryError::InvalidNode(DUMMY_PATH_STR.into(), ValidateNodeError::InvalidDigestLen(0)))
)]
#[case::directory_invalid_node_name_no_storepath(
    castorepb::node::Node::Directory(castorepb::DirectoryNode {
        name: "invalid".into(),
        digest: DUMMY_DIGEST.clone().into(),
        size: 0,
    }),
    ValidatePathInfoError::InvalidNodeName("invalid".into(), store_path::Error::InvalidLength)
)]
#[case::file_invalid_digest_len(
    castorepb::node::Node::File(castorepb::FileNode {
        name: DUMMY_PATH_STR.into(),
        digest: Bytes::new(),
        ..Default::default()
    }),
    ValidatePathInfoError::InvalidRootNode(DirectoryError::InvalidNode(DUMMY_PATH_STR.into(), ValidateNodeError::InvalidDigestLen(0)))
)]
#[case::file_invalid_node_name(
    castorepb::node::Node::File(castorepb::FileNode {
        name: "invalid".into(),
        digest: DUMMY_DIGEST.clone().into(),
        ..Default::default()
    }),
    ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength
    )
)]
#[case::symlink_invalid_node_name(
    castorepb::node::Node::Symlink(castorepb::SymlinkNode {
        name: "invalid".into(),
        target: "foo".into(),
    }),
    ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength
    )
)]
fn convert_fail_node(#[case] node: castorepb::node::Node, #[case] exp_err: ValidatePathInfoError) {
    // construct the proto::PathInfo object
    let mut p = PROTO_PATH_INFO.clone();
    p.node = Some(castorepb::Node { node: Some(node) });

    assert_eq!(exp_err, PathInfo::try_from(p).expect_err("must fail"));
}

/// Ensure a PathInfo without narinfo populated fails converting!
#[test]
fn convert_without_narinfo_fail() {
    let mut path_info = PROTO_PATH_INFO.clone();
    path_info.narinfo = None;

    assert_eq!(
        ValidatePathInfoError::NarInfoFieldMissing,
        PathInfo::try_from(path_info).expect_err("must fail"),
    );
}

/// Create a PathInfo with a wrong digest length in narinfo.nar_sha256, and
/// ensure conversion fails.
#[test]
fn convert_wrong_nar_sha256() {
    let mut path_info = PROTO_PATH_INFO.clone();
    path_info.narinfo.as_mut().unwrap().nar_sha256 = vec![0xbe, 0xef].into();

    assert_eq!(
        ValidatePathInfoError::InvalidNarSha256DigestLen(2),
        PathInfo::try_from(path_info).expect_err("must fail")
    );
}

/// Create a PathInfo with a wrong count of narinfo.reference_names,
/// and ensure validation fails.
#[test]
fn convert_inconsistent_num_refs_fail() {
    let mut path_info = PROTO_PATH_INFO.clone();
    path_info.narinfo.as_mut().unwrap().reference_names = vec![];

    assert_eq!(
        ValidatePathInfoError::InconsistentNumberOfReferences(1, 0),
        PathInfo::try_from(path_info).expect_err("must fail")
    );
}

/// Create a PathInfo with a wrong digest length in references.
#[test]
fn convert_invalid_reference_digest_len() {
    let mut path_info = PROTO_PATH_INFO.clone();
    path_info.references.push(vec![0xff, 0xff].into());

    assert_eq!(
        ValidatePathInfoError::InvalidReferenceDigestLen(
            1, // position
            2, // unexpected digest len
        ),
        PathInfo::try_from(path_info).expect_err("must fail")
    );
}

/// Create a PathInfo with a narinfo.reference_name[1] that is no valid store path.
#[test]
fn convert_invalid_narinfo_reference_name() {
    let mut path_info = PROTO_PATH_INFO.clone();

    // This is invalid, as the store prefix is not part of reference_names.
    path_info.narinfo.as_mut().unwrap().reference_names[0] =
        "/nix/store/00000000000000000000000000000000-dummy".to_string();

    assert_eq!(
        ValidatePathInfoError::InvalidNarinfoReferenceName(
            0,
            "/nix/store/00000000000000000000000000000000-dummy".to_string()
        ),
        PathInfo::try_from(path_info).expect_err("must fail")
    );
}

/// Create a PathInfo with a narinfo.reference_name[0] that doesn't match references[0].
#[test]
fn convert_inconsistent_narinfo_reference_name_digest() {
    let mut path_info = PROTO_PATH_INFO.clone();

    // mutate the first reference, they were all zeroes before
    path_info.references[0] = vec![0xff; store_path::DIGEST_SIZE].into();

    assert_eq!(
        ValidatePathInfoError::InconsistentNarinfoReferenceNameDigest(
            0,
            path_info.references[0][..].try_into().unwrap(),
            DUMMY_PATH_DIGEST
        ),
        PathInfo::try_from(path_info).expect_err("must fail")
    )
}

/// Create a PathInfo with a broken deriver field and ensure it fails.
#[test]
fn convert_invalid_deriver() {
    let mut path_info = PROTO_PATH_INFO.clone();

    // add a broken deriver (invalid digest)
    let narinfo = path_info.narinfo.as_mut().unwrap();
    narinfo.deriver = Some(crate::proto::StorePath {
        name: "foo".to_string(),
        digest: vec![].into(),
    });

    assert_eq!(
        ValidatePathInfoError::InvalidDeriverField(store_path::Error::InvalidLength),
        PathInfo::try_from(path_info).expect_err("must fail")
    )
}
