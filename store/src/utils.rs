use std::{ops::Deref, path::Path, sync::Arc};

use data_encoding::BASE64;
use nix_compat::store_path::{self, StorePath};
use tracing::{debug, instrument};
use tvix_castore::{
    blobservice::{self, BlobService},
    directoryservice::{self, DirectoryService},
    proto::node::Node,
};

use crate::{
    pathinfoservice::{self, PathInfoService},
    proto::{nar_info, NarInfo, PathInfo},
};

/// Construct the three store handles from their addrs.
pub async fn construct_services(
    blob_service_addr: impl AsRef<str>,
    directory_service_addr: impl AsRef<str>,
    path_info_service_addr: impl AsRef<str>,
) -> std::io::Result<(
    Arc<dyn BlobService>,
    Arc<dyn DirectoryService>,
    Box<dyn PathInfoService>,
)> {
    let blob_service: Arc<dyn BlobService> = blobservice::from_addr(blob_service_addr.as_ref())
        .await?
        .into();
    let directory_service: Arc<dyn DirectoryService> =
        directoryservice::from_addr(directory_service_addr.as_ref())
            .await?
            .into();
    let path_info_service = pathinfoservice::from_addr(
        path_info_service_addr.as_ref(),
        blob_service.clone(),
        directory_service.clone(),
    )
    .await?;

    Ok((blob_service, directory_service, path_info_service))
}

/// Imports a given path on the filesystem into the store, and returns the
/// [PathInfo] describing the path, that was sent to
/// [PathInfoService].
#[instrument(skip_all, fields(path=?path), err)]
pub async fn import_path<BS, DS, PS, P>(
    path: P,
    blob_service: BS,
    directory_service: DS,
    path_info_service: PS,
) -> Result<StorePath, std::io::Error>
where
    P: AsRef<Path> + std::fmt::Debug,
    BS: Deref<Target = dyn BlobService> + Clone,
    DS: Deref<Target = dyn DirectoryService> + Clone,
    PS: Deref<Target = dyn PathInfoService>,
{
    // calculate the name
    // TODO: make a path_to_name helper function?
    let name = path
        .as_ref()
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path must not be .. and the basename valid unicode",
            )
        })?;

    // Ingest the path into blob and directory service.
    let root_node = tvix_castore::import::ingest_path(blob_service, directory_service, &path)
        .await
        .expect("failed to ingest path");

    debug!(root_node =?root_node, "import successful");

    // Ask the PathInfoService for the NAR size and sha256
    let (nar_size, nar_sha256) = path_info_service.calculate_nar(&root_node).await?;

    // Calculate the output path. This might still fail, as some names are illegal.
    let output_path = store_path::build_nar_based_store_path(&nar_sha256, name).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid name: {}", name),
        )
    })?;

    // assemble a new root_node with a name that is derived from the nar hash.
    let root_node = root_node.rename(output_path.to_string().into_bytes().into());
    log_node(&root_node, path.as_ref());

    // assemble the [crate::proto::PathInfo] object.
    let path_info = PathInfo {
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
            ca: Some(nar_info::Ca {
                r#type: nar_info::ca::Hash::NarSha256.into(),
                digest: nar_sha256.to_vec().into(),
            }),
        }),
    };

    // put into [PathInfoService], and return the PathInfo that we get back
    // from there (it might contain additional signatures).
    let _path_info = path_info_service.put(path_info).await?;

    Ok(output_path.to_owned())
}

fn log_node(node: &Node, path: &Path) {
    match node {
        Node::Directory(directory_node) => {
            debug!(
                path = ?path,
                name = ?directory_node.name,
                digest = BASE64.encode(&directory_node.digest),
                "import successful",
            )
        }
        Node::File(file_node) => {
            debug!(
                path = ?path,
                name = ?file_node.name,
                digest = BASE64.encode(&file_node.digest),
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
