//! Implements builtins used to import paths in the store.

use crate::builtins::errors::ImportError;
use std::path::Path;
use tvix_castore::import::ingest_entries;
use tvix_eval::{
    builtin_macros::builtins,
    generators::{self, GenCo},
    ErrorKind, EvalIO, Value,
};

use std::rc::Rc;

async fn filtered_ingest(
    state: Rc<TvixStoreIO>,
    co: GenCo,
    path: &Path,
    filter: Option<&Value>,
) -> Result<tvix_castore::proto::node::Node, ErrorKind> {
    let mut entries: Vec<walkdir::DirEntry> = vec![];
    let mut it = walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(false)
        .into_iter();

    // Skip root node.
    entries.push(
        it.next()
            .ok_or_else(|| ErrorKind::IO {
                path: Some(path.to_path_buf()),
                error: std::io::Error::new(std::io::ErrorKind::NotFound, "No root node emitted")
                    .into(),
            })?
            .map_err(|err| ErrorKind::IO {
                path: Some(path.to_path_buf()),
                error: std::io::Error::from(err).into(),
            })?,
    );

    while let Some(entry) = it.next() {
        // Entry could be a NotFound, if the root path specified does not exist.
        let entry = entry.map_err(|err| ErrorKind::IO {
            path: err.path().map(|p| p.to_path_buf()),
            error: std::io::Error::from(err).into(),
        })?;

        // As per Nix documentation `:doc builtins.filterSource`.
        let file_type = if entry.file_type().is_dir() {
            "directory"
        } else if entry.file_type().is_file() {
            "regular"
        } else if entry.file_type().is_symlink() {
            "symlink"
        } else {
            "unknown"
        };

        let should_keep: bool = if let Some(filter) = filter {
            generators::request_force(
                &co,
                generators::request_call_with(
                    &co,
                    filter.clone(),
                    [
                        Value::String(entry.path().as_os_str().as_encoded_bytes().into()),
                        Value::String(file_type.into()),
                    ],
                )
                .await,
            )
            .await
            .as_bool()?
        } else {
            true
        };

        if !should_keep {
            if file_type == "directory" {
                it.skip_current_dir();
            }
            continue;
        }

        entries.push(entry);
    }

    let dir_entries = entries.into_iter().rev().map(Ok);

    state.tokio_handle.block_on(async {
        let entries = tvix_castore::import::fs::dir_entries_to_ingestion_stream(
            &state.blob_service,
            dir_entries,
            path,
        );
        ingest_entries(&state.directory_service, entries)
            .await
            .map_err(|e| ErrorKind::IO {
                path: Some(path.to_path_buf()),
                error: Rc::new(std::io::Error::new(std::io::ErrorKind::Other, e)),
            })
    })
}

#[builtins(state = "Rc<TvixStoreIO>")]
mod import_builtins {
    use std::os::unix::ffi::OsStrExt;
    use std::rc::Rc;

    use super::*;

    use crate::tvix_store_io::TvixStoreIO;
    use nix_compat::nixhash::{CAHash, NixHash};
    use nix_compat::store_path::StorePath;
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    use tvix_castore::proto::node::Node;
    use tvix_castore::proto::FileNode;
    use tvix_eval::builtins::coerce_value_to_path;
    use tvix_eval::generators::Gen;
    use tvix_eval::{generators::GenCo, ErrorKind, Value};
    use tvix_eval::{FileType, NixContextElement, NixString};

    #[builtin("path")]
    async fn builtin_path(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        let args = args.to_attrs()?;
        let path = args.select_required("path")?;
        let path =
            match coerce_value_to_path(&co, generators::request_force(&co, path.clone()).await)
                .await?
            {
                Ok(path) => path,
                Err(cek) => return Ok(cek.into()),
            };
        let name: String = if let Some(name) = args.select("name") {
            generators::request_force(&co, name.clone())
                .await
                .to_str()?
                .as_bstr()
                .to_string()
        } else {
            tvix_store::import::path_to_name(&path)
                .expect("Failed to derive the default name out of the path")
                .to_string()
        };
        let filter = args.select("filter");
        let recursive_ingestion = args
            .select("recursive")
            .map(|r| r.as_bool())
            .transpose()?
            .unwrap_or(true); // Yes, yes, Nix, by default, puts `recursive = true;`.
        let expected_sha256 = args
            .select("sha256")
            .map(|h| {
                h.to_str().and_then(|expected| {
                    let expected = expected.into_bstring().to_string();
                    // TODO: ensure that we fail if this is not a valid str.
                    nix_compat::nixhash::from_str(&expected, None).map_err(|_err| {
                        // TODO: a better error would be nice, we use
                        // DerivationError::InvalidOutputHash usually for derivation construction.
                        // This is not a derivation construction, should we move it outside and
                        // generalize?
                        ErrorKind::TypeError {
                            expected: "sha256",
                            actual: "not a sha256",
                        }
                    })
                })
            })
            .transpose()?;

        // Check if the path points to a regular file.
        // If it does, the filter function is never executed.
        // TODO: follow symlinks and check their type instead
        let (root_node, ca_hash) = match state.file_type(path.as_ref())? {
            FileType::Regular => {
                let mut file = state.open(path.as_ref())?;
                // This is a single file, copy it to the blobservice directly.
                let mut hash = sha2::Sha256::new();
                let mut blob_size = 0;
                let mut blob_writer = state
                    .tokio_handle
                    .block_on(async { state.blob_service.open_write().await });

                let mut buf = [0u8; 4096];

                loop {
                    // read bytes into buffer, break out if EOF
                    let len = file.read(&mut buf)?;
                    if len == 0 {
                        break;
                    }
                    blob_size += len as u64;

                    let data = &buf[0..len];

                    // add to blobwriter
                    state
                        .tokio_handle
                        .block_on(async { blob_writer.write_all(data).await })?;

                    // update the sha256 hash function. We can skip that if we're not using it.
                    if !recursive_ingestion {
                        hash.update(data);
                    }
                }

                // close the blob writer, get back the b3 digest.
                let blob_digest = state
                    .tokio_handle
                    .block_on(async { blob_writer.close().await })?;

                let root_node = Node::File(FileNode {
                    // The name gets set further down, while constructing the PathInfo.
                    name: "".into(),
                    digest: blob_digest.into(),
                    size: blob_size,
                    executable: false,
                });

                let ca_hash = if recursive_ingestion {
                    let (_nar_size, nar_sha256) = state
                        .tokio_handle
                        .block_on(async {
                            state
                                .nar_calculation_service
                                .as_ref()
                                .calculate_nar(&root_node)
                                .await
                        })
                        .map_err(|e| tvix_eval::ErrorKind::TvixError(Rc::new(e)))?;
                    CAHash::Nar(NixHash::Sha256(nar_sha256))
                } else {
                    CAHash::Flat(NixHash::Sha256(hash.finalize().into()))
                };

                (root_node, ca_hash)
            }

            FileType::Directory => {
                if !recursive_ingestion {
                    return Err(ImportError::FlatImportOfNonFile(
                        path.to_string_lossy().to_string(),
                    ))?;
                }

                // do the filtered ingest
                let root_node = filtered_ingest(state.clone(), co, path.as_ref(), filter).await?;

                // calculate the NAR sha256
                let (_nar_size, nar_sha256) = state
                    .tokio_handle
                    .block_on(async {
                        state
                            .nar_calculation_service
                            .as_ref()
                            .calculate_nar(&root_node)
                            .await
                    })
                    .map_err(|e| tvix_eval::ErrorKind::TvixError(Rc::new(e)))?;

                let ca_hash = CAHash::Nar(NixHash::Sha256(nar_sha256));

                (root_node, ca_hash)
            }
            FileType::Symlink => {
                // FUTUREWORK: Nix follows a symlink if it's at the root,
                // except if it's not resolve-able (NixOS/nix#7761).i
                return Err(tvix_eval::ErrorKind::IO {
                    path: Some(path.to_path_buf()),
                    error: Rc::new(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "builtins.path pointing to a symlink is ill-defined.",
                    )),
                });
            }
            FileType::Unknown => {
                return Err(tvix_eval::ErrorKind::IO {
                    path: Some(path.to_path_buf()),
                    error: Rc::new(std::io::Error::new(
                        std::io::ErrorKind::Unsupported,
                        "unsupported file type",
                    )),
                })
            }
        };

        let (path_info, _hash, output_path) = state.tokio_handle.block_on(async {
            state
                .node_to_path_info(name.as_ref(), path.as_ref(), &ca_hash, root_node)
                .await
        })?;

        if let Some(expected_sha256) = expected_sha256 {
            if *ca_hash.hash() != expected_sha256 {
                Err(ImportError::HashMismatch(
                    path.to_string_lossy().to_string(),
                    expected_sha256,
                    ca_hash.hash().into_owned(),
                ))?;
            }
        }

        state
            .tokio_handle
            .block_on(async { state.path_info_service.as_ref().put(path_info).await })
            .map_err(|e| tvix_eval::ErrorKind::IO {
                path: Some(path.to_path_buf()),
                error: Rc::new(e.into()),
            })?;

        // We need to attach context to the final output path.
        let outpath = output_path.to_absolute_path();

        Ok(
            NixString::new_context_from(NixContextElement::Plain(outpath.clone()).into(), outpath)
                .into(),
        )
    }

    #[builtin("filterSource")]
    async fn builtin_filter_source(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        #[lazy] filter: Value,
        path: Value,
    ) -> Result<Value, ErrorKind> {
        let p = path.to_path()?;
        let root_node = filtered_ingest(Rc::clone(&state), co, &p, Some(&filter)).await?;
        let name = tvix_store::import::path_to_name(&p)?;

        let outpath = state
            .tokio_handle
            .block_on(async {
                let (_, nar_sha256) = state
                    .nar_calculation_service
                    .as_ref()
                    .calculate_nar(&root_node)
                    .await?;

                state
                    .register_node_in_path_info_service(
                        name,
                        &p,
                        &CAHash::Nar(NixHash::Sha256(nar_sha256)),
                        root_node,
                    )
                    .await
            })
            .map_err(|err| ErrorKind::IO {
                path: Some(p.to_path_buf()),
                error: err.into(),
            })?
            .to_absolute_path();

        Ok(
            NixString::new_context_from(NixContextElement::Plain(outpath.clone()).into(), outpath)
                .into(),
        )
    }

    #[builtin("storePath")]
    async fn builtin_store_path(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        path: Value,
    ) -> Result<Value, ErrorKind> {
        let p = std::str::from_utf8(match &path {
            Value::String(s) => s.as_bytes(),
            Value::Path(p) => p.as_os_str().as_bytes(),
            _ => {
                return Err(ErrorKind::TypeError {
                    expected: "string or path",
                    actual: path.type_of(),
                })
            }
        })?;

        let path_exists = if let Ok((store_path, sub_path)) = StorePath::from_absolute_path_full(p)
        {
            if !sub_path.as_os_str().is_empty() {
                false
            } else {
                state.store_path_exists(store_path.as_ref()).await?
            }
        } else {
            false
        };

        if !path_exists {
            return Err(ImportError::PathNotInStore(p.into()).into());
        }

        Ok(Value::String(NixString::new_context_from(
            [NixContextElement::Plain(p.into())].into(),
            p,
        )))
    }
}

pub use import_builtins::builtins as import_builtins;

use crate::tvix_store_io::TvixStoreIO;
