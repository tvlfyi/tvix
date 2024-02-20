//! Implements builtins used to import paths in the store.

use futures::pin_mut;
use std::path::Path;
use tvix_eval::{
    builtin_macros::builtins,
    generators::{self, GenCo},
    ErrorKind, Value,
};

use std::rc::Rc;

async fn filtered_ingest(
    state: Rc<TvixStoreIO>,
    co: GenCo,
    path: &Path,
    filter: Option<&Value>,
) -> Result<tvix_castore::proto::node::Node, ErrorKind> {
    let mut entries_per_depths: Vec<Vec<walkdir::DirEntry>> = vec![Vec::new()];
    let mut it = walkdir::WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(false)
        .sort_by_file_name()
        .into_iter();

    // Skip root node.
    entries_per_depths[0].push(
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
                        Value::String(Box::new(entry.path().as_os_str().as_encoded_bytes().into())),
                        Value::String(Box::new(file_type.into())),
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

        if entry.depth() >= entries_per_depths.len() {
            debug_assert!(
                entry.depth() == entries_per_depths.len(),
                "Received unexpected entry with depth {} during descent, previously at {}",
                entry.depth(),
                entries_per_depths.len()
            );

            entries_per_depths.push(vec![entry]);
        } else {
            entries_per_depths[entry.depth()].push(entry);
        }

        // FUTUREWORK: determine when it's the right moment to flush a level to the ingester.
    }

    let entries_stream = tvix_castore::import::leveled_entries_to_stream(entries_per_depths);

    pin_mut!(entries_stream);

    state
        .ingest_entries_sync(entries_stream)
        .map_err(|err| ErrorKind::IO {
            path: Some(path.to_path_buf()),
            error: err.into(),
        })
}

#[builtins(state = "Rc<TvixStoreIO>")]
mod import_builtins {
    use std::rc::Rc;

    use super::*;

    use tvix_eval::generators::Gen;
    use tvix_eval::{generators::GenCo, ErrorKind, Value};

    use crate::tvix_store_io::TvixStoreIO;

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

        Ok(state
            .register_node_in_path_info_service_sync(name, &p, root_node)
            .map_err(|err| ErrorKind::IO {
                path: Some(p.to_path_buf()),
                error: err.into(),
            })?
            .to_absolute_path()
            .into())
    }
}

pub use import_builtins::builtins as import_builtins;

use crate::tvix_store_io::TvixStoreIO;
