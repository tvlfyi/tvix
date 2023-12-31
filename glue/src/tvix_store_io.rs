//! This module provides an implementation of EvalIO talking to tvix-store.

use nix_compat::store_path::StorePath;
use std::{
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::AsyncReadExt;
use tracing::{error, instrument, warn};
use tvix_eval::{EvalIO, FileType, StdIO};

use tvix_castore::{
    blobservice::BlobService,
    directoryservice::{self, DirectoryService},
    proto::node::Node,
    B3Digest,
};
use tvix_store::pathinfoservice::PathInfoService;

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
    /// [directoryservice::descend_to] to return the
    /// [Node] specified by `sub_path`.
    #[instrument(skip(self), ret, err)]
    fn store_path_to_root_node(
        &self,
        store_path: &StorePath,
        sub_path: &Path,
    ) -> io::Result<Option<Node>> {
        let path_info_service = self.path_info_service.clone();
        let task = self.tokio_handle.spawn({
            let digest = *store_path.digest();
            async move { path_info_service.get(digest).await }
        });
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
    fn path_exists(&self, path: &Path) -> io::Result<bool> {
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
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
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
    fn read_dir(&self, path: &Path) -> io::Result<Vec<(bytes::Bytes, FileType)>> {
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
    fn import_path(&self, path: &Path) -> io::Result<PathBuf> {
        let task = self.tokio_handle.spawn({
            let blob_service = self.blob_service.clone();
            let directory_service = self.directory_service.clone();
            let path_info_service = self.path_info_service.clone();
            let path = path.to_owned();
            async move {
                tvix_store::utils::import_path(
                    path,
                    blob_service,
                    directory_service,
                    path_info_service,
                )
                .await
            }
        });

        let output_path = self.tokio_handle.block_on(task)??;

        Ok(output_path.to_absolute_path().into())
    }

    #[instrument(skip(self), ret)]
    fn store_dir(&self) -> Option<String> {
        Some("/nix/store".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, path::Path, rc::Rc, sync::Arc};

    use tempfile::TempDir;
    use tvix_castore::{blobservice::MemoryBlobService, directoryservice::MemoryDirectoryService};
    use tvix_eval::EvaluationResult;
    use tvix_store::pathinfoservice::MemoryPathInfoService;

    use crate::{builtins::add_derivation_builtins, known_paths::KnownPaths};

    use super::TvixStoreIO;

    /// evaluates a given nix expression and returns the result.
    /// Takes care of setting up the evaluator so it knows about the
    // `derivation` builtin.
    fn eval(str: &str) -> EvaluationResult {
        let mut eval = tvix_eval::Evaluation::new_impure();

        let blob_service = Arc::new(MemoryBlobService::default());
        let directory_service = Arc::new(MemoryDirectoryService::default());
        let path_info_service = Arc::new(MemoryPathInfoService::new(
            blob_service.clone(),
            directory_service.clone(),
        ));
        let runtime = tokio::runtime::Runtime::new().unwrap();

        eval.io_handle = Box::new(TvixStoreIO::new(
            blob_service,
            directory_service,
            path_info_service,
            runtime.handle().clone(),
        ));

        let known_paths: Rc<RefCell<KnownPaths>> = Default::default();

        add_derivation_builtins(&mut eval, known_paths.clone());

        // run the evaluation itself.
        eval.evaluate(str, None)
    }

    /// Helper function that takes a &Path, and invokes a tvix evaluator coercing that path to a string
    /// (via "${/this/path}"). The path can be both absolute or not.
    /// It returns Option<String>, depending on whether the evaluation succeeded or not.
    fn import_path_and_compare<P: AsRef<Path>>(p: P) -> Option<String> {
        // Try to import the path using "${/tmp/path/to/test}".
        // The format string looks funny, the {} passed to Nix needs to be
        // escaped.
        let code = format!(r#""${{{}}}""#, p.as_ref().display());
        let result = eval(&code);

        if !result.errors.is_empty() {
            return None;
        }

        let value = result.value.expect("must be some");
        match value {
            tvix_eval::Value::String(s) => return Some(s.as_str().to_owned()),
            _ => panic!("unexpected value type: {:?}", value),
        }
    }

    /// Import a directory with a zero-sized ".keep" regular file.
    /// Ensure it matches the (pre-recorded) store path that Nix would produce.
    #[test]
    fn import_directory() {
        let tmpdir = TempDir::new().unwrap();

        // create a directory named "test"
        let src_path = tmpdir.path().join("test");
        std::fs::create_dir(&src_path).unwrap();

        // write a regular file `.keep`.
        std::fs::write(src_path.join(".keep"), vec![]).unwrap();

        // importing the path with .../test at the end.
        assert_eq!(
            Some("/nix/store/gq3xcv4xrj4yr64dflyr38acbibv3rm9-test".to_string()),
            import_path_and_compare(&src_path)
        );

        // importing the path with .../test/. at the end.
        assert_eq!(
            Some("/nix/store/gq3xcv4xrj4yr64dflyr38acbibv3rm9-test".to_string()),
            import_path_and_compare(src_path.join("."))
        );
    }

    /// Import a file into the store. Nix uses the "recursive"/NAR-based hashing
    /// scheme for these.
    #[test]
    fn import_file() {
        let tmpdir = TempDir::new().unwrap();

        // write a regular file `empty`.
        std::fs::write(tmpdir.path().join("empty"), vec![]).unwrap();

        assert_eq!(
            Some("/nix/store/lx5i78a4izwk2qj1nq8rdc07y8zrwy90-empty".to_string()),
            import_path_and_compare(tmpdir.path().join("empty"))
        );

        // write a regular file `hello.txt`.
        std::fs::write(tmpdir.path().join("hello.txt"), b"Hello World!").unwrap();

        assert_eq!(
            Some("/nix/store/925f1jb1ajrypjbyq7rylwryqwizvhp0-hello.txt".to_string()),
            import_path_and_compare(tmpdir.path().join("hello.txt"))
        );
    }

    /// Invoke toString on a nonexisting file, and access the .file attribute.
    /// This should not cause an error, because it shouldn't trigger an import,
    /// and leave the path as-is.
    #[test]
    fn nonexisting_path_without_import() {
        let result = eval("toString ({ line = 42; col = 42; file = /deep/thought; }.file)");

        assert!(result.errors.is_empty(), "expect evaluation to succeed");
        let value = result.value.expect("must be some");

        match value {
            tvix_eval::Value::String(s) => {
                assert_eq!("/deep/thought", s.as_str());
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }
}
