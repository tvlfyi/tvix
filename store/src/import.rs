use std::path::Path;
use tracing::{debug, instrument};
use tvix_castore::{
    blobservice::BlobService, directoryservice::DirectoryService, import::fs::ingest_path,
    proto::node::Node, B3Digest,
};

use nix_compat::{
    nixhash::{CAHash, NixHash},
    store_path::{self, StorePath},
};

use crate::{
    pathinfoservice::PathInfoService,
    proto::{nar_info, NarInfo, PathInfo},
};

impl From<CAHash> for nar_info::Ca {
    fn from(value: CAHash) -> Self {
        let hash_type: nar_info::ca::Hash = (&value).into();
        let digest: bytes::Bytes = value.hash().to_string().into();
        nar_info::Ca {
            r#type: hash_type.into(),
            digest,
        }
    }
}

pub fn log_node(node: &Node, path: &Path) {
    match node {
        Node::Directory(directory_node) => {
            debug!(
                path = ?path,
                name = ?directory_node.name,
                digest = %B3Digest::try_from(directory_node.digest.clone()).unwrap(),
                "import successful",
            )
        }
        Node::File(file_node) => {
            debug!(
                path = ?path,
                name = ?file_node.name,
                digest = %B3Digest::try_from(file_node.digest.clone()).unwrap(),
                "import successful"
            )
        }
        Node::Symlink(symlink_node) => {
            debug!(
                path = ?path,
                name = ?symlink_node.name,
                target = ?symlink_node.target,
                "import successful"
            )
        }
    }
}

/// Transform a path into its base name and returns an [`std::io::Error`] if it is `..` or if the
/// basename is not valid unicode.
#[inline]
pub fn path_to_name(path: &Path) -> std::io::Result<&str> {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path must not be .. and the basename valid unicode",
            )
        })
}

/// Takes the NAR size, SHA-256 of the NAR representation, the root node and optionally
/// a CA hash information.
///
/// Returns the path information object for a NAR-style object.
///
/// This [`PathInfo`] can be further filled for signatures, deriver or verified for the expected
/// hashes.
#[inline]
pub fn derive_nar_ca_path_info(
    nar_size: u64,
    nar_sha256: [u8; 32],
    ca: Option<CAHash>,
    root_node: Node,
) -> PathInfo {
    // assemble the [crate::proto::PathInfo] object.
    PathInfo {
        node: Some(tvix_castore::proto::Node {
            node: Some(root_node),
        }),
        // There's no reference scanning on path contents ingested like this.
        references: vec![],
        narinfo: Some(NarInfo {
            nar_size,
            nar_sha256: nar_sha256.to_vec().into(),
            signatures: vec![],
            reference_names: vec![],
            deriver: None,
            ca: ca.map(|ca_hash| ca_hash.into()),
        }),
    }
}

/// Ingest the given path `path` and register the resulting output path in the
/// [`PathInfoService`] as a recursive fixed output NAR.
#[instrument(skip_all, fields(store_name=name, path=?path), err)]
pub async fn import_path_as_nar_ca<BS, DS, PS, P>(
    path: P,
    name: &str,
    blob_service: BS,
    directory_service: DS,
    path_info_service: PS,
) -> Result<StorePath, std::io::Error>
where
    P: AsRef<Path> + std::fmt::Debug,
    BS: BlobService + Clone,
    DS: AsRef<dyn DirectoryService>,
    PS: AsRef<dyn PathInfoService>,
{
    let root_node = ingest_path(blob_service, directory_service, path.as_ref())
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Ask the PathInfoService for the NAR size and sha256
    let (nar_size, nar_sha256) = path_info_service.as_ref().calculate_nar(&root_node).await?;

    // Calculate the output path. This might still fail, as some names are illegal.
    // FUTUREWORK: express the `name` at the type level to be valid and move the conversion
    // at the caller level.
    let output_path = store_path::build_nar_based_store_path(&nar_sha256, name).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid name: {}", name),
        )
    })?;

    // assemble a new root_node with a name that is derived from the nar hash.
    let root_node = root_node.rename(output_path.to_string().into_bytes().into());
    log_node(&root_node, path.as_ref());

    let path_info = derive_nar_ca_path_info(
        nar_size,
        nar_sha256,
        Some(CAHash::Nar(NixHash::Sha256(nar_sha256))),
        root_node,
    );

    // This new [`PathInfo`] that we get back from there might contain additional signatures or
    // information set by the service itself. In this function, we silently swallow it because
    // callers doesn't really need it.
    let _path_info = path_info_service.as_ref().put(path_info).await?;

    Ok(output_path.to_owned())
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, path::PathBuf};

    use crate::import::path_to_name;
    use rstest::rstest;

    #[rstest]
    #[case::simple_path("a/b/c", "c")]
    #[case::simple_path_containing_dotdot("a/b/../c", "c")]
    #[case::path_containing_multiple_dotdot("a/b/../c/d/../e", "e")]

    fn test_path_to_name(#[case] path: &str, #[case] expected_name: &str) {
        let path: PathBuf = path.into();
        assert_eq!(path_to_name(&path).expect("must succeed"), expected_name);
    }

    #[rstest]
    #[case::path_ending_in_dotdot(b"a/b/..")]
    #[case::non_unicode_path(b"\xf8\xa1\xa1\xa1\xa1")]
    fn test_invalid_path_to_name(#[case] invalid_path: &[u8]) {
        let path: PathBuf = unsafe { OsStr::from_encoded_bytes_unchecked(invalid_path) }.into();
        path_to_name(&path).expect_err("must fail");
    }
}
