//! Implements builtins used to import paths in the store.

use crate::builtins::errors::ImportError;
use std::path::Path;
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

    let entries_iter = entries.into_iter().rev().map(Ok);

    state.tokio_handle.block_on(async {
        state
            .ingest_dir_entries(entries_iter, path)
            .await
            .map_err(|err| ErrorKind::IO {
                path: Some(path.to_path_buf()),
                error: err.into(),
            })
    })
}

#[builtins(state = "Rc<TvixStoreIO>")]
mod import_builtins {
    use std::rc::Rc;

    use super::*;

    use nix_compat::nixhash::{CAHash, NixHash};
    use tvix_eval::generators::Gen;
    use tvix_eval::{generators::GenCo, ErrorKind, Value};
    use tvix_eval::{NixContextElement, NixString};

    use tvix_castore::B3Digest;

    use crate::tvix_store_io::TvixStoreIO;

    #[builtin("path")]
    async fn builtin_path(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        let args = args.to_attrs()?;
        let path = args.select_required("path")?;
        let path = generators::request_force(&co, path.clone())
            .await
            .to_path()?;
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

        // FUTUREWORK(performance): this opens the file instead of using a stat-like
        // system call to the file.
        if !recursive_ingestion && state.open(path.as_ref()).is_err() {
            Err(ImportError::FlatImportOfNonFile(
                path.to_string_lossy().to_string(),
            ))?;
        }

        let root_node = filtered_ingest(state.clone(), co, path.as_ref(), filter).await?;
        let ca: CAHash = if recursive_ingestion {
            CAHash::Nar(NixHash::Sha256(state.tokio_handle.block_on(async {
                Ok::<_, tvix_eval::ErrorKind>(
                    state
                        .path_info_service
                        .as_ref()
                        .calculate_nar(&root_node)
                        .await
                        .map_err(|e| ErrorKind::TvixError(Rc::new(e)))?
                        .1,
                )
            })?))
        } else {
            let digest: B3Digest = match root_node {
                tvix_castore::proto::node::Node::File(ref fnode) => {
                    // It's already validated.
                    fnode.digest.clone().try_into().unwrap()
                }
                // We cannot hash anything else than file in flat import mode.
                _ => {
                    return Err(ImportError::FlatImportOfNonFile(
                        path.to_string_lossy().to_string(),
                    )
                    .into())
                }
            };

            // FUTUREWORK: avoid hashing again.
            CAHash::Flat(NixHash::Sha256(
                state
                    .tokio_handle
                    .block_on(async { state.blob_to_sha256_hash(digest).await })?,
            ))
        };

        let obtained_hash = ca.hash().clone().into_owned();
        let (path_info, _hash, output_path) = state.tokio_handle.block_on(async {
            state
                .node_to_path_info(name.as_ref(), path.as_ref(), ca, root_node)
                .await
        })?;

        if let Some(expected_sha256) = expected_sha256 {
            if obtained_hash != expected_sha256 {
                Err(ImportError::HashMismatch(
                    path.to_string_lossy().to_string(),
                    expected_sha256,
                    obtained_hash,
                ))?;
            }
        }

        let _: tvix_store::proto::PathInfo = state.tokio_handle.block_on(async {
            // This is necessary to cause the coercion of the error type.
            Ok::<_, std::io::Error>(state.path_info_service.as_ref().put(path_info).await?)
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
                    .path_info_service
                    .as_ref()
                    .calculate_nar(&root_node)
                    .await?;

                state
                    .register_node_in_path_info_service(
                        name,
                        &p,
                        CAHash::Nar(NixHash::Sha256(nar_sha256)),
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
}

pub use import_builtins::builtins as import_builtins;

use crate::tvix_store_io::TvixStoreIO;
