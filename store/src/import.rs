use bstr::ByteSlice;
use std::path::Path;
use tracing::{debug, instrument};
use tvix_castore::{
    blobservice::BlobService, directoryservice::DirectoryService, import::fs::ingest_path, Node,
};

use nix_compat::{
    nixhash::{CAHash, NixHash},
    store_path::{self, StorePath},
};

use crate::{
    nar::NarCalculationService,
    pathinfoservice::{PathInfo, PathInfoService},
    proto::nar_info,
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

pub fn log_node(name: &[u8], node: &Node, path: &Path) {
    match node {
        Node::Directory { digest, .. } => {
            debug!(
                path = ?path,
                name = %name.as_bstr(),
                digest = %digest,
                "import successful",
            )
        }
        Node::File { digest, .. } => {
            debug!(
                path = ?path,
                name = %name.as_bstr(),
                digest = %digest,
                "import successful"
            )
        }
        Node::Symlink { target } => {
            debug!(
                path = ?path,
                name = %name.as_bstr(),
                target = ?target,
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

/// Ingest the contents at the given path `path` into castore, and registers the
/// resulting root node in the passed PathInfoService, using the "NAR sha256
/// digest" and the passed name for output path calculation.
/// Inserts the PathInfo into the PathInfoService and returns it back to the caller.
#[instrument(skip_all, fields(store_name=name, path=?path), err)]
pub async fn import_path_as_nar_ca<BS, DS, PS, NS, P>(
    path: P,
    name: &str,
    blob_service: BS,
    directory_service: DS,
    path_info_service: PS,
    nar_calculation_service: NS,
) -> Result<PathInfo, std::io::Error>
where
    P: AsRef<Path> + std::fmt::Debug,
    BS: BlobService + Clone,
    DS: DirectoryService,
    PS: AsRef<dyn PathInfoService>,
    NS: NarCalculationService,
{
    // Ingest the contents at the given path `path` into castore.
    let root_node =
        ingest_path::<_, _, _, &[u8]>(blob_service, directory_service, path.as_ref(), None)
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Ask for the NAR size and sha256
    let (nar_size, nar_sha256) = nar_calculation_service.calculate_nar(&root_node).await?;

    // Calculate the output path. This might still fail, as some names are illegal.
    // FUTUREWORK: express the `name` at the type level to be valid and move the conversion
    // at the caller level.
    let output_path: StorePath<String> = store_path::build_nar_based_store_path(&nar_sha256, name)
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid name: {}", name),
            )
        })?;

    // Insert a PathInfo. On success, return it back to the caller.
    Ok(path_info_service
        .as_ref()
        .put(PathInfo {
            store_path: output_path.to_owned(),
            node: root_node,
            // There's no reference scanning on imported paths
            references: vec![],
            nar_size,
            nar_sha256,
            signatures: vec![],
            deriver: None,
            ca: Some(CAHash::Nar(NixHash::Sha256(nar_sha256))),
        })
        .await?)
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
