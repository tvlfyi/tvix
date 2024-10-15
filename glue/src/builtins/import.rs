//! Implements builtins used to import paths in the store.

use crate::tvix_store_io::TvixStoreIO;
use std::path::Path;
use tvix_castore::import::ingest_entries;
use tvix_castore::Node;
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
) -> Result<Node, ErrorKind> {
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
        let entries = tvix_castore::import::fs::dir_entries_to_ingestion_stream::<'_, _, _, &[u8]>(
            &state.blob_service,
            dir_entries,
            path,
            None, // TODO re-scan
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
    use super::*;

    use crate::builtins::ImportError;
    use crate::tvix_store_io::TvixStoreIO;
    use bstr::ByteSlice;
    use nix_compat::nixhash::{CAHash, NixHash};
    use nix_compat::store_path::{build_ca_path, StorePathRef};
    use sha2::Digest;
    use std::rc::Rc;
    use tokio::io::AsyncWriteExt;
    use tvix_eval::builtins::coerce_value_to_path;
    use tvix_eval::generators::Gen;
    use tvix_eval::{generators::GenCo, ErrorKind, Value};
    use tvix_eval::{FileType, NixContextElement, NixString};
    use tvix_store::path_info::PathInfo;

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

        // Construct a sha256 hasher, which is needed for flat ingestion.
        let recursive_ingestion = args
            .select("recursive")
            .map(|r| r.as_bool())
            .transpose()?
            .unwrap_or(true); // Yes, yes, Nix, by default, puts `recursive = true;`.

        let expected_sha256 = args
            .select("sha256")
            .map(|h| {
                h.to_str().and_then(|expected| {
                    match nix_compat::nixhash::from_str(
                        expected.into_bstring().to_str()?,
                        Some("sha256"),
                    ) {
                        Ok(NixHash::Sha256(digest)) => Ok(digest),
                        Ok(_) => unreachable!(),
                        Err(_e) => {
                            // TODO: a better error would be nice, we use
                            // DerivationError::InvalidOutputHash usually for derivation construction.
                            // This is not a derivation construction, should we move it outside and
                            // generalize?
                            Err(ErrorKind::TypeError {
                                expected: "sha256",
                                actual: "not a sha256",
                            })
                        }
                    }
                })
            })
            .transpose()?;

        // As a first step, we ingest the contents, and get back a root node,
        // and optionally the sha256 a flat file.
        let (root_node, ca) = match state.file_type(path.as_ref())? {
            // Check if the path points to a regular file.
            // If it does, the filter function is never executed, and we copy to the blobservice directly.
            // If recursive is false, we need to calculate the sha256 digest of the raw contents,
            // as that affects the output path calculation.
            FileType::Regular => {
                let mut file = state.open(path.as_ref())?;

                let mut flat_sha256 = (!recursive_ingestion).then(sha2::Sha256::new);
                let mut blob_size = 0;

                let mut blob_writer = state
                    .tokio_handle
                    .block_on(async { state.blob_service.open_write().await });

                // read piece by piece and write to blob_writer.
                // This is a bit manual due to EvalIO being sync, while everything else async.
                {
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

                        // update blob_sha256 if needed.
                        if let Some(h) = flat_sha256.as_mut() {
                            h.update(data)
                        }
                    }
                }

                // close the blob writer, construct the root node and the blob_sha256 (later used for output path calculation)
                (
                    Node::File {
                        digest: state
                            .tokio_handle
                            .block_on(async { blob_writer.close().await })?,
                        size: blob_size,
                        executable: false,
                    },
                    {
                        // If non-recursive ingestion is requested…
                        if let Some(flat_sha256) = flat_sha256 {
                            let actual_sha256 = flat_sha256.finalize().into();

                            // compare the recorded flat hash with an upfront one if provided.
                            if let Some(expected_sha256) = expected_sha256 {
                                if actual_sha256 != expected_sha256 {
                                    return Err(ImportError::HashMismatch(
                                        path,
                                        NixHash::Sha256(expected_sha256),
                                        NixHash::Sha256(actual_sha256),
                                    )
                                    .into());
                                }
                            }

                            Some(CAHash::Flat(NixHash::Sha256(actual_sha256)))
                        } else {
                            None
                        }
                    },
                )
            }

            FileType::Directory if !recursive_ingestion => {
                return Err(ImportError::FlatImportOfNonFile(path))?
            }

            // do the filtered ingest
            FileType::Directory => (
                filtered_ingest(state.clone(), co, path.as_ref(), filter).await?,
                None,
            ),
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

        // Calculate the NAR sha256.
        let (nar_size, nar_sha256) = state
            .tokio_handle
            .block_on(async {
                state
                    .nar_calculation_service
                    .as_ref()
                    .calculate_nar(&root_node)
                    .await
            })
            .map_err(|e| tvix_eval::ErrorKind::TvixError(Rc::new(e)))?;

        // Calculate the CA hash for the recursive cases, this is only already
        // `Some(_)` for flat ingestion.
        let ca = match ca {
            None => {
                // If an upfront-expected NAR hash was specified, compare.
                if let Some(expected_nar_sha256) = expected_sha256 {
                    if expected_nar_sha256 != nar_sha256 {
                        return Err(ImportError::HashMismatch(
                            path,
                            NixHash::Sha256(expected_nar_sha256),
                            NixHash::Sha256(nar_sha256),
                        )
                        .into());
                    }
                }
                CAHash::Nar(NixHash::Sha256(nar_sha256))
            }
            Some(ca) => ca,
        };

        let store_path = build_ca_path(&name, &ca, Vec::<&str>::new(), false)
            .map_err(|e| tvix_eval::ErrorKind::TvixError(Rc::new(e)))?;

        let path_info = state
            .tokio_handle
            .block_on(async {
                state
                    .path_info_service
                    .as_ref()
                    .put(PathInfo {
                        store_path,
                        node: root_node,
                        // There's no reference scanning on path contents ingested like this.
                        references: vec![],
                        nar_size,
                        nar_sha256,
                        signatures: vec![],
                        deriver: None,
                        ca: Some(ca),
                    })
                    .await
            })
            .map_err(|e| tvix_eval::ErrorKind::IO {
                path: Some(path.to_path_buf()),
                error: Rc::new(e.into()),
            })?;

        // We need to attach context to the final output path.
        let outpath = path_info.store_path.to_absolute_path();

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

        let path_info = state
            .tokio_handle
            .block_on(async {
                // Ask the PathInfoService for the NAR size and sha256
                // We always need it no matter what is the actual hash mode
                // because the [PathInfo] needs to contain nar_{sha256,size}.
                let (nar_size, nar_sha256) = state
                    .nar_calculation_service
                    .as_ref()
                    .calculate_nar(&root_node)
                    .await?;

                let ca = CAHash::Nar(NixHash::Sha256(nar_sha256));

                // Calculate the output path. This might still fail, as some names are illegal.
                let output_path =
                    nix_compat::store_path::build_ca_path(name, &ca, Vec::<&str>::new(), false)
                        .map_err(|_| {
                            std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("invalid name: {}", name),
                            )
                        })?;

                state
                    .path_info_service
                    .as_ref()
                    .put(PathInfo {
                        store_path: output_path,
                        node: root_node,
                        // There's no reference scanning on path contents ingested like this.
                        references: vec![],
                        nar_size,
                        nar_sha256,
                        signatures: vec![],
                        deriver: None,
                        ca: Some(ca),
                    })
                    .await
            })
            .map_err(|e| ErrorKind::IO {
                path: Some(p.to_path_buf()),
                error: Rc::new(e.into()),
            })?;

        // We need to attach context to the final output path.
        let outpath = path_info.store_path.to_absolute_path();

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
        let p = match &path {
            Value::String(s) => Path::new(s.as_bytes().to_os_str()?),
            Value::Path(p) => p.as_path(),
            _ => {
                return Err(ErrorKind::TypeError {
                    expected: "string or path",
                    actual: path.type_of(),
                })
            }
        };

        // For this builtin, the path needs to start with an absolute store path.
        let (store_path, _sub_path) = StorePathRef::from_absolute_path_full(p)
            .map_err(|_e| ImportError::PathNotAbsoluteOrInvalid(p.to_path_buf()))?;

        if state.path_exists(p)? {
            Ok(Value::String(NixString::new_context_from(
                [NixContextElement::Plain(store_path.to_absolute_path())].into(),
                p.as_os_str().as_encoded_bytes(),
            )))
        } else {
            Err(ErrorKind::IO {
                path: Some(p.to_path_buf()),
                error: Rc::new(std::io::ErrorKind::NotFound.into()),
            })
        }
    }
}

pub use import_builtins::builtins as import_builtins;
