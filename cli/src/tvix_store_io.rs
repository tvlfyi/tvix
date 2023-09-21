//! This module provides an implementation of EvalIO talking to tvix-store.

use nix_compat::store_path::{self, StorePath};
use std::{io, path::Path, path::PathBuf, sync::Arc};
use tokio::io::AsyncReadExt;
use tracing::{error, instrument, warn};
use tvix_eval::{EvalIO, FileType, StdIO};

use tvix_store::{
    blobservice::BlobService,
    directoryservice::{self, DirectoryService},
    import,
    nar::calculate_size_and_sha256,
    pathinfoservice::PathInfoService,
    proto::{node::Node, NamedNode, NarInfo, PathInfo},
    B3Digest,
};

/// Implements [EvalIO], asking given [PathInfoService], [DirectoryService]
/// and [BlobService].
///
/// In case the given path does not exist in these stores, we ask StdIO.
/// This is to both cover cases of syntactically valid store paths, that exist
/// on the filesystem (still managed by Nix), as well as being able to read
/// files outside store paths.
pub struct TvixStoreIO {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,
    std_io: StdIO,
    tokio_handle: tokio::runtime::Handle,
}

impl TvixStoreIO {
    pub fn new(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
        path_info_service: Arc<dyn PathInfoService>,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            blob_service,
            directory_service,
            path_info_service,
            std_io: StdIO {},
            tokio_handle,
        }
    }

    /// for a given [StorePath] and additional [Path] inside the store path,
    /// look up the [PathInfo], and if it exists, and then use
    /// [directoryservice::traverse_to] to return the
    /// [Node] specified by `sub_path`.
    #[instrument(skip(self), ret, err)]
    fn store_path_to_root_node(
        &self,
        store_path: &StorePath,
        sub_path: &Path,
    ) -> Result<Option<Node>, io::Error> {
        let path_info_service = self.path_info_service.clone();
        let digest = store_path.digest.clone();
        let task = self
            .tokio_handle
            .spawn(async move { path_info_service.get(digest).await });
        let path_info = match self.tokio_handle.block_on(task).unwrap()? {
            // If there's no PathInfo found, early exit
            None => return Ok(None),
            Some(path_info) => path_info,
        };

        let root_node = {
            match path_info.node {
                None => {
                    warn!(
                        "returned PathInfo {:?} node is None, this shouldn't happen.",
                        &path_info
                    );
                    return Ok(None);
                }
                Some(root_node) => match root_node.node {
                    None => {
                        warn!("node for {:?} is None, this shouldn't happen.", &root_node);
                        return Ok(None);
                    }
                    Some(root_node) => root_node,
                },
            }
        };

        let directory_service = self.directory_service.clone();
        let sub_path = sub_path.to_owned();
        let task = self.tokio_handle.spawn(async move {
            directoryservice::descend_to(directory_service, root_node, &sub_path).await
        });

        Ok(self.tokio_handle.block_on(task).unwrap()?)
    }
}

impl EvalIO for TvixStoreIO {
    #[instrument(skip(self), ret, err)]
    fn path_exists(&self, path: &Path) -> Result<bool, io::Error> {
        if let Ok((store_path, sub_path)) =
            StorePath::from_absolute_path_full(&path.to_string_lossy())
        {
            if self
                .store_path_to_root_node(&store_path, &sub_path)?
                .is_some()
            {
                Ok(true)
            } else {
                // As tvix-store doesn't manage /nix/store on the filesystem,
                // we still need to also ask self.std_io here.
                self.std_io.path_exists(path)
            }
        } else {
            // The store path is no store path, so do regular StdIO.
            self.std_io.path_exists(path)
        }
    }

    #[instrument(skip(self), ret, err)]
    fn read_to_string(&self, path: &Path) -> Result<String, io::Error> {
        if let Ok((store_path, sub_path)) =
            StorePath::from_absolute_path_full(&path.to_string_lossy())
        {
            if let Some(node) = self.store_path_to_root_node(&store_path, &sub_path)? {
                // depending on the node type, treat read_to_string differently
                match node {
                    Node::Directory(_) => {
                        // This would normally be a io::ErrorKind::IsADirectory (still unstable)
                        Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            format!("tried to read directory at {:?} to string", path),
                        ))
                    }
                    Node::File(file_node) => {
                        let digest: B3Digest =
                            file_node.digest.clone().try_into().map_err(|_e| {
                                error!(
                                    file_node = ?file_node,
                                    "invalid digest"
                                );
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!("invalid digest length in file node: {:?}", file_node),
                                )
                            })?;

                        let blob_service = self.blob_service.clone();

                        let task = self.tokio_handle.spawn(async move {
                            let mut reader = {
                                let resp = blob_service.open_read(&digest).await?;
                                match resp {
                                    Some(blob_reader) => blob_reader,
                                    None => {
                                        error!(
                                            blob.digest = %digest,
                                            "blob not found",
                                        );
                                        Err(io::Error::new(
                                            io::ErrorKind::NotFound,
                                            format!("blob {} not found", &digest),
                                        ))?
                                    }
                                }
                            };

                            let mut buf = String::new();

                            reader.read_to_string(&mut buf).await?;
                            Ok(buf)
                        });

                        self.tokio_handle.block_on(task).unwrap()
                    }
                    Node::Symlink(_symlink_node) => Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "read_to_string for symlinks is unsupported",
                    ))?,
                }
            } else {
                // As tvix-store doesn't manage /nix/store on the filesystem,
                // we still need to also ask self.std_io here.
                self.std_io.read_to_string(path)
            }
        } else {
            // The store path is no store path, so do regular StdIO.
            self.std_io.read_to_string(path)
        }
    }

    #[instrument(skip(self), ret, err)]
    fn read_dir(&self, path: &Path) -> Result<Vec<(bytes::Bytes, FileType)>, io::Error> {
        if let Ok((store_path, sub_path)) =
            StorePath::from_absolute_path_full(&path.to_string_lossy())
        {
            if let Some(node) = self.store_path_to_root_node(&store_path, &sub_path)? {
                match node {
                    Node::Directory(directory_node) => {
                        // fetch the Directory itself.
                        let digest: B3Digest =
                            directory_node.digest.clone().try_into().map_err(|_e| {
                                io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    format!(
                                        "invalid digest length in directory node: {:?}",
                                        directory_node
                                    ),
                                )
                            })?;

                        let directory_service = self.directory_service.clone();
                        let digest_clone = digest.clone();
                        let task = self
                            .tokio_handle
                            .spawn(async move { directory_service.get(&digest_clone).await });
                        if let Some(directory) = self.tokio_handle.block_on(task).unwrap()? {
                            let mut children: Vec<(bytes::Bytes, FileType)> = Vec::new();
                            for node in directory.nodes() {
                                children.push(match node {
                                    Node::Directory(e) => (e.name, FileType::Directory),
                                    Node::File(e) => (e.name, FileType::Regular),
                                    Node::Symlink(e) => (e.name, FileType::Symlink),
                                })
                            }
                            Ok(children)
                        } else {
                            // If we didn't get the directory node that's linked, that's a store inconsistency!
                            error!(
                                directory.digest = %digest,
                                path = ?path,
                                "directory not found",
                            );
                            Err(io::Error::new(
                                io::ErrorKind::NotFound,
                                format!("directory {digest} does not exist"),
                            ))?
                        }
                    }
                    Node::File(_file_node) => {
                        // This would normally be a io::ErrorKind::NotADirectory (still unstable)
                        Err(io::Error::new(
                            io::ErrorKind::Unsupported,
                            "tried to readdir path {:?}, which is a file",
                        ))?
                    }
                    Node::Symlink(_symlink_node) => Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "read_dir for symlinks is unsupported",
                    ))?,
                }
            } else {
                self.std_io.read_dir(path)
            }
        } else {
            self.std_io.read_dir(path)
        }
    }

    #[instrument(skip(self), ret, err)]
    fn import_path(&self, path: &std::path::Path) -> Result<PathBuf, std::io::Error> {
        let p = path.to_owned();
        let blob_service = self.blob_service.clone();
        let directory_service = self.directory_service.clone();
        let path_info_service = self.path_info_service.clone();

        let task = self.tokio_handle.spawn(async move {
            import_path_with_pathinfo(blob_service, directory_service, path_info_service, &p).await
        });

        let path_info = self.tokio_handle.block_on(task).unwrap()?;

        // from the [PathInfo], extract the store path (as string).
        Ok({
            let mut path = PathBuf::from(nix_compat::store_path::STORE_DIR_WITH_SLASH);

            let root_node_name = path_info.node.unwrap().node.unwrap().get_name().to_vec();

            // This must be a string, otherwise it would have failed validation.
            let root_node_name = String::from_utf8(root_node_name).unwrap();

            // append to the PathBuf
            path.push(root_node_name);

            // and return it
            path
        })
    }

    #[instrument(skip(self), ret)]
    fn store_dir(&self) -> Option<String> {
        Some("/nix/store".to_string())
    }
}

/// Imports a given path on the filesystem into the store, and returns the
/// [PathInfo] describing the path, that was sent to
/// [PathInfoService].
#[instrument(skip(blob_service, directory_service, path_info_service), ret, err)]
async fn import_path_with_pathinfo(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,
    path: &std::path::Path,
) -> Result<PathInfo, io::Error> {
    // Call [import::ingest_path], which will walk over the given path and return a root_node.
    let root_node = import::ingest_path(blob_service.clone(), directory_service.clone(), path)
        .await
        .expect("error during import_path");

    // Render the NAR.
    let (nar_size, nar_sha256) =
        calculate_size_and_sha256(&root_node, blob_service.clone(), directory_service.clone())
            .await
            .expect("error during nar calculation"); // TODO: handle error

    // TODO: make a path_to_name helper function?
    let name = path
        .file_name()
        .expect("path must not be ..")
        .to_str()
        .expect("path must be valid unicode");

    let output_path = store_path::build_nar_based_store_path(&nar_sha256, name);

    // assemble a new root_node with a name that is derived from the nar hash.
    let root_node = root_node.rename(output_path.to_string().into_bytes().into());

    // assemble the [PathInfo] object.
    let path_info = PathInfo {
        node: Some(tvix_store::proto::Node {
            node: Some(root_node),
        }),
        // There's no reference scanning on path contents ingested like this.
        references: vec![],
        narinfo: Some(NarInfo {
            nar_size,
            nar_sha256: nar_sha256.to_vec().into(),
            signatures: vec![],
            reference_names: vec![],
            // TODO: narinfo for talosctl.src contains `CA: fixed:r:sha256:1x13j5hy75221bf6kz7cpgld9vgic6bqx07w5xjs4pxnksj6lxb6`
            // do we need this anywhere?
        }),
    };

    // put into [PathInfoService], and return the [PathInfo] that we get
    // back from there (it might contain additional signatures).
    let path_info = path_info_service.put(path_info).await?;

    Ok(path_info)
}
