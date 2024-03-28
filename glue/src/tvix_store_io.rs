//! This module provides an implementation of EvalIO talking to tvix-store.

use async_recursion::async_recursion;
use bytes::Bytes;
use futures::Stream;
use futures::{StreamExt, TryStreamExt};
use nix_compat::nixhash::NixHash;
use nix_compat::store_path::{build_ca_path, StorePathRef};
use nix_compat::{nixhash::CAHash, store_path::StorePath};
use sha2::{Digest, Sha256};
use std::marker::Unpin;
use std::rc::Rc;
use std::{
    cell::RefCell,
    collections::BTreeSet,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::AsyncReadExt;
use tracing::{error, instrument, warn, Level};
use tvix_build::buildservice::BuildService;
use tvix_eval::{ErrorKind, EvalIO, FileType, StdIO};
use walkdir::DirEntry;

use tvix_castore::{
    blobservice::BlobService,
    directoryservice::{self, DirectoryService},
    proto::{node::Node, FileNode, NamedNode},
    B3Digest,
};
use tvix_store::{pathinfoservice::PathInfoService, proto::PathInfo};

use crate::builtins::FetcherError;
use crate::known_paths::KnownPaths;
use crate::tvix_build::derivation_to_build_request;

/// Implements [EvalIO], asking given [PathInfoService], [DirectoryService]
/// and [BlobService].
///
/// In case the given path does not exist in these stores, we ask StdIO.
/// This is to both cover cases of syntactically valid store paths, that exist
/// on the filesystem (still managed by Nix), as well as being able to read
/// files outside store paths.
///
/// This structure is also directly used by the derivation builtins
/// and tightly coupled to it.
///
/// In the future, we may revisit that coupling and figure out how to generalize this interface and
/// hide this implementation detail of the glue itself so that glue can be used with more than one
/// implementation of "Tvix Store IO" which does not necessarily bring the concept of blob service,
/// directory service or path info service.
pub struct TvixStoreIO {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    // This is public so builtins can put PathInfos directly.
    pub(crate) path_info_service: Arc<dyn PathInfoService>,
    std_io: StdIO,
    #[allow(dead_code)]
    build_service: Arc<dyn BuildService>,
    pub(crate) tokio_handle: tokio::runtime::Handle,
    http_client: reqwest::Client,
    pub(crate) known_paths: RefCell<KnownPaths>,
}

impl TvixStoreIO {
    pub fn new(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
        path_info_service: Arc<dyn PathInfoService>,
        build_service: Arc<dyn BuildService>,
        tokio_handle: tokio::runtime::Handle,
    ) -> Self {
        Self {
            blob_service,
            directory_service,
            path_info_service,
            std_io: StdIO {},
            build_service,
            tokio_handle,
            http_client: reqwest::Client::new(),
            known_paths: Default::default(),
        }
    }

    /// for a given [StorePath] and additional [Path] inside the store path,
    /// look up the [PathInfo], and if it exists, and then use
    /// [directoryservice::descend_to] to return the
    /// [Node] specified by `sub_path`.
    ///
    /// In case there is no PathInfo yet, this means we need to build it
    /// (which currently is stubbed out still).
    #[async_recursion(?Send)]
    #[instrument(skip(self, store_path), fields(store_path=%store_path), ret(level = Level::TRACE), err)]
    async fn store_path_to_node(
        &self,
        store_path: &StorePath,
        sub_path: &Path,
    ) -> io::Result<Option<Node>> {
        // Find the root node for the store_path.
        // It asks the PathInfoService first, but in case there was a Derivation
        // produced that would build it, fall back to triggering the build.
        // To populate the input nodes, it might recursively trigger builds of
        // its dependencies too.
        let root_node = match self
            .path_info_service
            .as_ref()
            .get(*store_path.digest())
            .await?
        {
            // if we have a PathInfo, we know there will be a root_node (due to validation)
            Some(path_info) => path_info.node.expect("no node").node.expect("no node"),
            // If there's no PathInfo found, this normally means we have to
            // trigger the build (and insert into PathInfoService, after
            // reference scanning).
            // However, as Tvix is (currently) not managing /nix/store itself,
            // we return Ok(None) to let std_io take over.
            // While reading from store paths that are not known to Tvix during
            // that evaluation clearly is an impurity, we still need to support
            // it for things like <nixpkgs> pointing to a store path.
            // In the future, these things will (need to) have PathInfo.
            None => {
                // The store path doesn't exist yet, so we need to build it.
                warn!("triggering build");

                // Look up the derivation for this output path.
                let (drv_path, drv) = {
                    let known_paths = self.known_paths.borrow();
                    match known_paths.get_drv_path_for_output_path(store_path) {
                        Some(drv_path) => (
                            drv_path.to_owned(),
                            known_paths.get_drv_by_drvpath(drv_path).unwrap().to_owned(),
                        ),
                        None => {
                            warn!(store_path=%store_path, "no drv found");
                            // let StdIO take over
                            return Ok(None);
                        }
                    }
                };

                // derivation_to_build_request needs castore nodes for all inputs.
                // Provide them, which means, here is where we recursively build
                // all dependencies.
                #[allow(clippy::mutable_key_type)]
                let input_nodes: BTreeSet<Node> =
                    futures::stream::iter(drv.input_derivations.iter())
                        .map(|(input_drv_path, output_names)| {
                            // look up the derivation object
                            let input_drv = {
                                let known_paths = self.known_paths.borrow();
                                known_paths
                                    .get_drv_by_drvpath(input_drv_path)
                                    .unwrap_or_else(|| panic!("{} not found", input_drv_path))
                                    .to_owned()
                            };

                            // convert output names to actual paths
                            let output_paths: Vec<StorePath> = output_names
                                .iter()
                                .map(|output_name| {
                                    input_drv
                                        .outputs
                                        .get(output_name)
                                        .expect("missing output_name")
                                        .path
                                        .as_ref()
                                        .expect("missing output path")
                                        .clone()
                                })
                                .collect();
                            // For each output, ask for the castore node.
                            // We're in a per-derivation context, so if they're
                            // not built yet they'll all get built together.
                            // If they don't need to build, we can however still
                            // substitute all in parallel (if they don't need to
                            // be built) - so we turn this into a stream of streams.
                            // It's up to the builder to deduplicate same build requests.
                            futures::stream::iter(output_paths.into_iter()).map(
                                |output_path| async move {
                                    let node = self
                                        .store_path_to_node(&output_path, Path::new(""))
                                        .await?;

                                    if let Some(node) = node {
                                        Ok(node)
                                    } else {
                                        Err(io::Error::other("no node produced"))
                                    }
                                },
                            )
                        })
                        .flatten()
                        .buffer_unordered(10) // TODO: make configurable
                        .try_collect()
                        .await?;

                // TODO: check if input sources are sufficiently dealth with,
                // I think yes, they must be imported into the store by other
                // operations, so dealt with in the Some(…) match arm

                // synthesize the build request.
                let build_request = derivation_to_build_request(&drv, input_nodes)?;

                // create a build
                let build_result = self
                    .build_service
                    .as_ref()
                    .do_build(build_request)
                    .await
                    .map_err(|e| std::io::Error::new(io::ErrorKind::Other, e))?;

                // TODO: refscan?

                // For each output, insert a PathInfo.
                for output in &build_result.outputs {
                    let root_node = output.node.as_ref().expect("invalid root node");

                    // calculate the nar representation
                    let (nar_size, nar_sha256) =
                        self.path_info_service.calculate_nar(root_node).await?;

                    // assemble the PathInfo to persist
                    let path_info = PathInfo {
                        node: Some(tvix_castore::proto::Node {
                            node: Some(root_node.clone()),
                        }),
                        references: vec![], // TODO: refscan
                        narinfo: Some(tvix_store::proto::NarInfo {
                            nar_size,
                            nar_sha256: Bytes::from(nar_sha256.to_vec()),
                            signatures: vec![],
                            reference_names: vec![], // TODO: refscan
                            deriver: Some(tvix_store::proto::StorePath {
                                name: drv_path
                                    .name()
                                    .strip_suffix(".drv")
                                    .expect("missing .drv suffix")
                                    .to_string(),
                                digest: drv_path.digest().to_vec().into(),
                            }),
                            ca: drv.fod_digest().map(
                                |fod_digest| -> tvix_store::proto::nar_info::Ca {
                                    (&CAHash::Nar(nix_compat::nixhash::NixHash::Sha256(fod_digest)))
                                        .into()
                                },
                            ),
                        }),
                    };

                    self.path_info_service
                        .put(path_info)
                        .await
                        .map_err(|e| std::io::Error::new(io::ErrorKind::Other, e))?;
                }

                // find the output for the store path requested
                build_result
                    .outputs
                    .into_iter()
                    .find(|output_node| {
                        output_node.node.as_ref().expect("invalid node").get_name()
                            == store_path.to_string().as_bytes()
                    })
                    .expect("build didn't produce the store path")
                    .node
                    .expect("invalid node")
            }
        };

        // now with the root_node and sub_path, descend to the node requested.
        directoryservice::descend_to(&self.directory_service, root_node, sub_path)
            .await
            .map_err(|e| std::io::Error::new(io::ErrorKind::Other, e))
    }

    /// This forwards the ingestion to the [`tvix_castore::import::ingest_entries`],
    /// passing the blob_service and directory_service that's used.
    /// The error is mapped to std::io::Error for simplicity.
    pub(crate) async fn ingest_entries<S>(&self, entries_stream: S) -> io::Result<Node>
    where
        S: Stream<Item = DirEntry> + Unpin,
    {
        tvix_castore::import::ingest_entries(
            &self.blob_service,
            &self.directory_service,
            entries_stream,
        )
        .await
        .map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))
    }

    pub(crate) async fn node_to_path_info(
        &self,
        name: &str,
        path: &Path,
        ca: CAHash,
        root_node: Node,
    ) -> io::Result<(PathInfo, StorePath)> {
        // Ask the PathInfoService for the NAR size and sha256
        // We always need it no matter what is the actual hash mode
        // because the path info construct a narinfo which *always*
        // require a SHA256 of the NAR representation and the NAR size.
        let (nar_size, nar_sha256) = self
            .path_info_service
            .as_ref()
            .calculate_nar(&root_node)
            .await?;

        // Calculate the output path. This might still fail, as some names are illegal.
        let output_path =
            nix_compat::store_path::build_ca_path(name, &ca, Vec::<String>::new(), false).map_err(
                |_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("invalid name: {}", name),
                    )
                },
            )?;

        // assemble a new root_node with a name that is derived from the nar hash.
        let root_node = root_node.rename(output_path.to_string().into_bytes().into());
        tvix_store::import::log_node(&root_node, path);

        let path_info =
            tvix_store::import::derive_nar_ca_path_info(nar_size, nar_sha256, Some(ca), root_node);

        Ok((path_info, output_path.to_owned()))
    }

    pub(crate) async fn register_node_in_path_info_service(
        &self,
        name: &str,
        path: &Path,
        ca: CAHash,
        root_node: Node,
    ) -> io::Result<StorePath> {
        let (path_info, output_path) = self.node_to_path_info(name, path, ca, root_node).await?;
        let _path_info = self.path_info_service.as_ref().put(path_info).await?;

        Ok(output_path)
    }

    pub async fn store_path_exists<'a>(&'a self, store_path: StorePathRef<'a>) -> io::Result<bool> {
        Ok(self
            .path_info_service
            .as_ref()
            .get(*store_path.digest())
            .await?
            .is_some())
    }

    pub async fn fetch_url(
        &self,
        url: &str,
        name: &str,
        hash: Option<&NixHash>,
    ) -> Result<StorePath, ErrorKind> {
        let resp = self
            .http_client
            .get(url)
            .send()
            .await
            .map_err(FetcherError::from)?;
        let mut sha = Sha256::new();
        let mut data = tokio_util::io::StreamReader::new(
            resp.bytes_stream()
                .inspect_ok(|data| {
                    sha.update(data);
                })
                .map_err(|e| {
                    let e = e.without_url();
                    warn!(%e, "failed to get response body");
                    io::Error::new(io::ErrorKind::BrokenPipe, e.to_string())
                }),
        );

        let mut blob = self.blob_service.open_write().await;
        let size = tokio::io::copy(&mut data, blob.as_mut()).await?;
        let blob_digest = blob.close().await?;
        let got = NixHash::Sha256(sha.finalize().into());

        let hash = CAHash::Flat(if let Some(wanted) = hash {
            if *wanted != got {
                return Err(FetcherError::HashMismatch {
                    url: url.to_owned(),
                    wanted: wanted.clone(),
                    got,
                }
                .into());
            }
            wanted.clone()
        } else {
            got
        });

        let path = build_ca_path(name, &hash, Vec::<String>::new(), false)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let node = Node::File(FileNode {
            name: path.to_string().into(),
            digest: blob_digest.into(),
            size,
            executable: false,
        });

        let (nar_size, nar_sha256) = self
            .path_info_service
            .calculate_nar(&node)
            .await
            .map_err(|e| ErrorKind::TvixError(Rc::new(e)))?;

        let path_info = PathInfo {
            node: Some(tvix_castore::proto::Node {
                node: Some(node.clone()),
            }),
            references: vec![],
            narinfo: Some(tvix_store::proto::NarInfo {
                nar_size,
                nar_sha256: nar_sha256.to_vec().into(),
                signatures: vec![],
                reference_names: vec![],
                deriver: None, /* ? */
                ca: Some((&hash).into()),
            }),
        };

        self.path_info_service
            .put(path_info)
            .await
            .map_err(|e| std::io::Error::new(io::ErrorKind::Other, e))?;

        Ok(path.to_owned())
    }
}

impl EvalIO for TvixStoreIO {
    #[instrument(skip(self), ret(level = Level::TRACE), err)]
    fn path_exists(&self, path: &Path) -> io::Result<bool> {
        if let Ok((store_path, sub_path)) =
            StorePath::from_absolute_path_full(&path.to_string_lossy())
        {
            if self
                .tokio_handle
                .block_on(async { self.store_path_to_node(&store_path, &sub_path).await })?
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

    #[instrument(skip(self), err)]
    fn read_to_end(&self, path: &Path) -> io::Result<Vec<u8>> {
        if let Ok((store_path, sub_path)) =
            StorePath::from_absolute_path_full(&path.to_string_lossy())
        {
            if let Some(node) = self
                .tokio_handle
                .block_on(async { self.store_path_to_node(&store_path, &sub_path).await })?
            {
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

                        self.tokio_handle.block_on(async {
                            let mut reader = {
                                let resp = self.blob_service.as_ref().open_read(&digest).await?;
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

                            let mut buf = Vec::new();

                            reader.read_to_end(&mut buf).await?;
                            Ok(buf)
                        })
                    }
                    Node::Symlink(_symlink_node) => Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "read_to_string for symlinks is unsupported",
                    ))?,
                }
            } else {
                // As tvix-store doesn't manage /nix/store on the filesystem,
                // we still need to also ask self.std_io here.
                self.std_io.read_to_end(path)
            }
        } else {
            // The store path is no store path, so do regular StdIO.
            self.std_io.read_to_end(path)
        }
    }

    #[instrument(skip(self), ret(level = Level::TRACE), err)]
    fn read_dir(&self, path: &Path) -> io::Result<Vec<(bytes::Bytes, FileType)>> {
        if let Ok((store_path, sub_path)) =
            StorePath::from_absolute_path_full(&path.to_string_lossy())
        {
            if let Some(node) = self
                .tokio_handle
                .block_on(async { self.store_path_to_node(&store_path, &sub_path).await })?
            {
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

                        if let Some(directory) = self.tokio_handle.block_on(async {
                            self.directory_service.as_ref().get(&digest).await
                        })? {
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

    #[instrument(skip(self), ret(level = Level::TRACE), err)]
    fn import_path(&self, path: &Path) -> io::Result<PathBuf> {
        let output_path = self.tokio_handle.block_on(async {
            tvix_store::import::import_path_as_nar_ca(
                path,
                tvix_store::import::path_to_name(path)?,
                &self.blob_service,
                &self.directory_service,
                &self.path_info_service,
            )
            .await
        })?;

        Ok(output_path.to_absolute_path().into())
    }

    #[instrument(skip(self), ret(level = Level::TRACE))]
    fn store_dir(&self) -> Option<String> {
        Some("/nix/store".to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, rc::Rc, sync::Arc};

    use bstr::ByteSlice;
    use tempfile::TempDir;
    use tvix_build::buildservice::DummyBuildService;
    use tvix_castore::{
        blobservice::{BlobService, MemoryBlobService},
        directoryservice::{DirectoryService, MemoryDirectoryService},
    };
    use tvix_eval::{EvalIO, EvaluationResult};
    use tvix_store::pathinfoservice::MemoryPathInfoService;

    use super::TvixStoreIO;
    use crate::builtins::{add_derivation_builtins, add_fetcher_builtins, add_import_builtins};

    /// evaluates a given nix expression and returns the result.
    /// Takes care of setting up the evaluator so it knows about the
    // `derivation` builtin.
    fn eval(str: &str) -> EvaluationResult {
        let blob_service = Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>;
        let directory_service =
            Arc::new(MemoryDirectoryService::default()) as Arc<dyn DirectoryService>;
        let path_info_service = Arc::new(MemoryPathInfoService::new(
            blob_service.clone(),
            directory_service.clone(),
        ));

        let runtime = tokio::runtime::Runtime::new().unwrap();

        let io = Rc::new(TvixStoreIO::new(
            blob_service.clone(),
            directory_service.clone(),
            path_info_service,
            Arc::<DummyBuildService>::default(),
            runtime.handle().clone(),
        ));
        let mut eval = tvix_eval::Evaluation::new(io.clone() as Rc<dyn EvalIO>, true);

        add_derivation_builtins(&mut eval, io.clone());
        add_fetcher_builtins(&mut eval, io.clone());
        add_import_builtins(&mut eval, io);

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
            tvix_eval::Value::String(s) => Some(s.to_str_lossy().into_owned()),
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
                assert_eq!(*s, "/deep/thought");
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }
}
