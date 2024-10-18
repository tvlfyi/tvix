use anyhow::Context;
use bstr::BStr;
use oci_spec::runtime::{LinuxIdMapping, LinuxIdMappingBuilder};
use tokio::process::{Child, Command};
use tonic::async_trait;
use tracing::{debug, instrument, warn, Span};
use tvix_castore::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    fs::fuse::FuseDaemon,
    import::fs::ingest_path,
    refscan::{ReferencePattern, ReferenceScanner},
    Node, PathComponent,
};
use uuid::Uuid;

use crate::{
    oci::{get_host_output_paths, make_bundle, make_spec},
    proto::{build::OutputNeedles, Build, BuildRequest},
};
use std::{collections::BTreeMap, ffi::OsStr, path::PathBuf, process::Stdio};

use super::BuildService;

const SANDBOX_SHELL: &str = env!("TVIX_BUILD_SANDBOX_SHELL");
const MAX_CONCURRENT_BUILDS: usize = 2; // TODO: make configurable

pub struct OCIBuildService<BS, DS> {
    /// Root path in which all bundles are created in
    bundle_root: PathBuf,

    /// uid mappings to set up for the workloads
    uid_mappings: Vec<LinuxIdMapping>,
    /// uid mappings to set up for the workloads
    gid_mappings: Vec<LinuxIdMapping>,

    /// Handle to a [BlobService], used by filesystems spawned during builds.
    blob_service: BS,
    /// Handle to a [DirectoryService], used by filesystems spawned during builds.
    directory_service: DS,

    // semaphore to track number of concurrently running builds.
    // this is necessary, as otherwise we very quickly run out of open file handles.
    concurrent_builds: tokio::sync::Semaphore,
}

impl<BS, DS> OCIBuildService<BS, DS> {
    pub fn new(bundle_root: PathBuf, blob_service: BS, directory_service: DS) -> Self {
        // We map root inside the container to the uid/gid this is running at,
        // and allocate one for uid 1000 into the container from the range we
        // got in /etc/sub{u,g}id.
        // TODO: actually read uid, and /etc/subuid. Maybe only when we try to build?
        // FUTUREWORK: use different uids?
        Self {
            bundle_root,
            blob_service,
            directory_service,
            uid_mappings: vec![
                LinuxIdMappingBuilder::default()
                    .host_id(1000_u32)
                    .container_id(0_u32)
                    .size(1_u32)
                    .build()
                    .unwrap(),
                LinuxIdMappingBuilder::default()
                    .host_id(100000_u32)
                    .container_id(1000_u32)
                    .size(1_u32)
                    .build()
                    .unwrap(),
            ],
            gid_mappings: vec![
                LinuxIdMappingBuilder::default()
                    .host_id(100_u32)
                    .container_id(0_u32)
                    .size(1_u32)
                    .build()
                    .unwrap(),
                LinuxIdMappingBuilder::default()
                    .host_id(100000_u32)
                    .container_id(100_u32)
                    .size(1_u32)
                    .build()
                    .unwrap(),
            ],
            concurrent_builds: tokio::sync::Semaphore::new(MAX_CONCURRENT_BUILDS),
        }
    }
}

#[async_trait]
impl<BS, DS> BuildService for OCIBuildService<BS, DS>
where
    BS: BlobService + Clone + 'static,
    DS: DirectoryService + Clone + 'static,
{
    #[instrument(skip_all, err)]
    async fn do_build(&self, request: BuildRequest) -> std::io::Result<Build> {
        let _permit = self.concurrent_builds.acquire().await.unwrap();

        let bundle_name = Uuid::new_v4();
        let bundle_path = self.bundle_root.join(bundle_name.to_string());

        let span = Span::current();
        span.record("bundle_name", bundle_name.to_string());

        let mut runtime_spec = make_spec(&request, true, SANDBOX_SHELL)
            .context("failed to create spec")
            .map_err(std::io::Error::other)?;

        let mut linux = runtime_spec.linux().clone().unwrap();

        // edit the spec, we need to setup uid/gid mappings.
        linux.set_uid_mappings(Some(self.uid_mappings.clone()));
        linux.set_gid_mappings(Some(self.gid_mappings.clone()));

        runtime_spec.set_linux(Some(linux));

        make_bundle(&request, &runtime_spec, &bundle_path)
            .context("failed to produce bundle")
            .map_err(std::io::Error::other)?;

        // pre-calculate the locations we want to later ingest, in the order of
        // the original outputs.
        // If we can't find calculate that path, don't start the build in first place.
        let host_output_paths = get_host_output_paths(&request, &bundle_path)
            .context("failed to calculate host output paths")
            .map_err(std::io::Error::other)?;

        // assemble a BTreeMap of Nodes to pass into TvixStoreFs.
        let root_nodes: BTreeMap<PathComponent, Node> =
            BTreeMap::from_iter(request.inputs.iter().map(|input| {
                // We know from validation this is Some.
                input.clone().into_name_and_node().unwrap()
            }));
        let patterns = ReferencePattern::new(request.refscan_needles.clone());
        // NOTE: impl Drop for FuseDaemon unmounts, so if the call is cancelled, umount.
        let _fuse_daemon = tokio::task::spawn_blocking({
            let blob_service = self.blob_service.clone();
            let directory_service = self.directory_service.clone();

            debug!(inputs=?root_nodes.keys(), "got inputs");

            let dest = bundle_path.join("inputs");

            move || {
                let fs = tvix_castore::fs::TvixStoreFs::new(
                    blob_service,
                    directory_service,
                    Box::new(root_nodes),
                    true,
                    false,
                );
                // mount the filesystem and wait for it to be unmounted.
                // FUTUREWORK: make fuse daemon threads configurable?
                FuseDaemon::new(fs, dest, 4, true).context("failed to start fuse daemon")
            }
        })
        .await?
        .context("mounting")
        .map_err(std::io::Error::other)?;

        debug!(bundle.path=?bundle_path, bundle.name=%bundle_name, "about to spawn bundle");

        // start the bundle as another process.
        let child = spawn_bundle(bundle_path, &bundle_name.to_string())?;

        // wait for the process to exit
        // FUTUREWORK: change the trait to allow reporting progress / logs…
        let child_output = child
            .wait_with_output()
            .await
            .context("failed to run process")
            .map_err(std::io::Error::other)?;

        // Check the exit code
        if !child_output.status.success() {
            let stdout = BStr::new(&child_output.stdout);
            let stderr = BStr::new(&child_output.stderr);

            warn!(stdout=%stdout, stderr=%stderr, exit_code=%child_output.status, "build failed");

            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "nonzero exit code".to_string(),
            ));
        }

        // Ingest build outputs into the castore.
        // We use try_join_all here. No need to spawn new tasks, as this is
        // mostly IO bound.
        let (outputs, outputs_needles) = futures::future::try_join_all(
            host_output_paths.into_iter().enumerate().map(|(i, p)| {
                let output_path = request.outputs[i].clone();
                let patterns = patterns.clone();
                async move {
                    debug!(host.path=?p, output.path=?output_path, "ingesting path");

                    let scanner = ReferenceScanner::new(patterns);
                    let output_node = ingest_path(
                        self.blob_service.clone(),
                        &self.directory_service,
                        p,
                        Some(&scanner),
                    )
                    .await
                    .map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Unable to ingest output: {}", e),
                        )
                    })?;

                    let needles = OutputNeedles {
                        needles: scanner
                            .matches()
                            .into_iter()
                            .enumerate()
                            .filter(|(_, val)| *val)
                            .map(|(idx, _)| idx as u64)
                            .collect(),
                    };

                    Ok::<_, std::io::Error>((
                        tvix_castore::proto::Node::from_name_and_node(
                            PathBuf::from(output_path)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .map(|s| s.to_string())
                                .unwrap_or("".into())
                                .into(),
                            output_node,
                        ),
                        needles,
                    ))
                }
            }),
        )
        .await?
        .into_iter()
        .unzip();

        Ok(Build {
            build_request: Some(request.clone()),
            outputs,
            outputs_needles,
        })
    }
}

/// Spawns runc with the bundle at bundle_path.
/// On success, returns the child.
#[instrument(err)]
fn spawn_bundle(
    bundle_path: impl AsRef<OsStr> + std::fmt::Debug,
    bundle_name: &str,
) -> std::io::Result<Child> {
    let mut command = Command::new("runc");

    command
        .args(&[
            "run".into(),
            "--bundle".into(),
            bundle_path.as_ref().to_os_string(),
            bundle_name.into(),
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .stdin(Stdio::null());

    command.spawn()
}
