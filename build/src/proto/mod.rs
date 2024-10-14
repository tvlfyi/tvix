use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use itertools::Itertools;
use tvix_castore::{DirectoryError, Node, PathComponent};

mod grpc_buildservice_wrapper;

pub use grpc_buildservice_wrapper::GRPCBuildServiceWrapper;

tonic::include_proto!("tvix.build.v1");

#[cfg(feature = "tonic-reflection")]
/// Compiled file descriptors for implementing [gRPC
/// reflection](https://github.com/grpc/grpc/blob/master/doc/server-reflection.md) with e.g.
/// [`tonic_reflection`](https://docs.rs/tonic-reflection).
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("tvix.build.v1");

/// Errors that occur during the validation of [BuildRequest] messages.
#[derive(Debug, thiserror::Error)]
pub enum ValidateBuildRequestError {
    #[error("invalid input node at position {0}: {1}")]
    InvalidInputNode(usize, DirectoryError),

    #[error("input nodes are not sorted by name")]
    InputNodesNotSorted,

    #[error("invalid working_dir")]
    InvalidWorkingDir,

    #[error("scratch_paths not sorted")]
    ScratchPathsNotSorted,

    #[error("invalid scratch path at position {0}")]
    InvalidScratchPath(usize),

    #[error("invalid inputs_dir")]
    InvalidInputsDir,

    #[error("invalid output path at position {0}")]
    InvalidOutputPath(usize),

    #[error("outputs not sorted")]
    OutputsNotSorted,

    #[error("invalid environment variable at position {0}")]
    InvalidEnvVar(usize),

    #[error("EnvVar not sorted by their keys")]
    EnvVarNotSorted,

    #[error("invalid build constraints: {0}")]
    InvalidBuildConstraints(ValidateBuildConstraintsError),

    #[error("invalid additional file path at position: {0}")]
    InvalidAdditionalFilePath(usize),

    #[error("additional_files not sorted")]
    AdditionalFilesNotSorted,
}

/// Checks a path to be without any '..' components, and clean (no superfluous
/// slashes).
fn is_clean_path<P: AsRef<Path>>(p: P) -> bool {
    let p = p.as_ref();

    // Look at all components, bail in case of ".", ".." and empty normal
    // segments (superfluous slashes)
    // We still need to assemble a cleaned PathBuf, and compare the OsString
    // later, as .components() already does do some normalization before
    // yielding.
    let mut cleaned_p = PathBuf::new();
    for component in p.components() {
        match component {
            std::path::Component::Prefix(_) => {}
            std::path::Component::RootDir => {}
            std::path::Component::CurDir => return false,
            std::path::Component::ParentDir => return false,
            std::path::Component::Normal(a) => {
                if a.is_empty() {
                    return false;
                }
            }
        }
        cleaned_p.push(component);
    }

    // if cleaned_p looks like p, we're good.
    if cleaned_p.as_os_str() != p.as_os_str() {
        return false;
    }

    true
}

fn is_clean_relative_path<P: AsRef<Path>>(p: P) -> bool {
    if p.as_ref().is_absolute() {
        return false;
    }

    is_clean_path(p)
}

fn is_clean_absolute_path<P: AsRef<Path>>(p: P) -> bool {
    if !p.as_ref().is_absolute() {
        return false;
    }

    is_clean_path(p)
}

/// Checks if a given list is sorted.
fn is_sorted<I>(data: I) -> bool
where
    I: Iterator,
    I::Item: Ord + Clone,
{
    data.tuple_windows().all(|(a, b)| a <= b)
}

impl BuildRequest {
    /// Ensures the build request is valid.
    /// This means, all input nodes need to be valid, paths in lists need to be sorted,
    /// and all restrictions around paths themselves (relative, clean, …) need
    // to be fulfilled.
    pub fn validate(&self) -> Result<(), ValidateBuildRequestError> {
        // validate names. Make sure they're sorted

        let mut last_name: bytes::Bytes = "".into();
        for (i, node) in self.inputs.iter().enumerate() {
            // TODO(flokli): store result somewhere
            let (name, _node) = node
                .clone()
                .into_name_and_node()
                .map_err(|e| ValidateBuildRequestError::InvalidInputNode(i, e))?;

            if name.as_ref() <= last_name.as_ref() {
                return Err(ValidateBuildRequestError::InputNodesNotSorted);
            } else {
                last_name = name.into()
            }
        }

        // validate working_dir
        if !is_clean_relative_path(&self.working_dir) {
            Err(ValidateBuildRequestError::InvalidWorkingDir)?;
        }

        // validate scratch paths
        for (i, p) in self.scratch_paths.iter().enumerate() {
            if !is_clean_relative_path(p) {
                Err(ValidateBuildRequestError::InvalidScratchPath(i))?
            }
        }
        if !is_sorted(self.scratch_paths.iter().map(|e| e.as_bytes())) {
            Err(ValidateBuildRequestError::ScratchPathsNotSorted)?;
        }

        // validate inputs_dir
        if !is_clean_relative_path(&self.inputs_dir) {
            Err(ValidateBuildRequestError::InvalidInputsDir)?;
        }

        // validate outputs
        for (i, p) in self.outputs.iter().enumerate() {
            if !is_clean_relative_path(p) {
                Err(ValidateBuildRequestError::InvalidOutputPath(i))?
            }
        }
        if !is_sorted(self.outputs.iter().map(|e| e.as_bytes())) {
            Err(ValidateBuildRequestError::OutputsNotSorted)?;
        }

        // validate environment_vars.
        for (i, e) in self.environment_vars.iter().enumerate() {
            if e.key.is_empty() || e.key.contains('=') {
                Err(ValidateBuildRequestError::InvalidEnvVar(i))?
            }
        }
        if !is_sorted(self.environment_vars.iter().map(|e| e.key.as_bytes())) {
            Err(ValidateBuildRequestError::EnvVarNotSorted)?;
        }

        // validate build constraints
        if let Some(constraints) = self.constraints.as_ref() {
            constraints
                .validate()
                .map_err(ValidateBuildRequestError::InvalidBuildConstraints)?;
        }

        // validate additional_files
        for (i, additional_file) in self.additional_files.iter().enumerate() {
            if !is_clean_relative_path(&additional_file.path) {
                Err(ValidateBuildRequestError::InvalidAdditionalFilePath(i))?
            }
        }
        if !is_sorted(self.additional_files.iter().map(|e| e.path.as_bytes())) {
            Err(ValidateBuildRequestError::AdditionalFilesNotSorted)?;
        }

        Ok(())
    }
}

impl TryFrom<BuildRequest> for crate::buildservice::BuildRequest {
    type Error = ValidateBuildRequestError;
    fn try_from(value: BuildRequest) -> Result<Self, Self::Error> {
        // validate input names. Make sure they're sorted

        let mut last_name: bytes::Bytes = "".into();
        let mut inputs: BTreeMap<PathComponent, Node> = BTreeMap::new();
        for (i, node) in value.inputs.iter().enumerate() {
            let (name, node) = node
                .clone()
                .into_name_and_node()
                .map_err(|e| ValidateBuildRequestError::InvalidInputNode(i, e))?;

            if name.as_ref() <= last_name.as_ref() {
                return Err(ValidateBuildRequestError::InputNodesNotSorted);
            } else {
                inputs.insert(name.clone(), node);
                last_name = name.into();
            }
        }

        // validate working_dir
        if !is_clean_relative_path(&value.working_dir) {
            Err(ValidateBuildRequestError::InvalidWorkingDir)?;
        }

        // validate scratch paths
        for (i, p) in value.scratch_paths.iter().enumerate() {
            if !is_clean_relative_path(p) {
                Err(ValidateBuildRequestError::InvalidScratchPath(i))?
            }
        }
        if !is_sorted(value.scratch_paths.iter().map(|e| e.as_bytes())) {
            Err(ValidateBuildRequestError::ScratchPathsNotSorted)?;
        }

        // validate inputs_dir
        if !is_clean_relative_path(&value.inputs_dir) {
            Err(ValidateBuildRequestError::InvalidInputsDir)?;
        }

        // validate outputs
        for (i, p) in value.outputs.iter().enumerate() {
            if !is_clean_relative_path(p) {
                Err(ValidateBuildRequestError::InvalidOutputPath(i))?
            }
        }
        if !is_sorted(value.outputs.iter().map(|e| e.as_bytes())) {
            Err(ValidateBuildRequestError::OutputsNotSorted)?;
        }

        // validate environment_vars.
        for (i, e) in value.environment_vars.iter().enumerate() {
            if e.key.is_empty() || e.key.contains('=') {
                Err(ValidateBuildRequestError::InvalidEnvVar(i))?
            }
        }
        if !is_sorted(value.environment_vars.iter().map(|e| e.key.as_bytes())) {
            Err(ValidateBuildRequestError::EnvVarNotSorted)?;
        }

        // validate build constraints
        let constraints = value
            .constraints
            .map_or(Ok(HashSet::new()), |constraints| {
                constraints
                    .try_into()
                    .map_err(ValidateBuildRequestError::InvalidBuildConstraints)
            })?;

        // validate additional_files
        for (i, additional_file) in value.additional_files.iter().enumerate() {
            if !is_clean_relative_path(&additional_file.path) {
                Err(ValidateBuildRequestError::InvalidAdditionalFilePath(i))?
            }
        }
        if !is_sorted(value.additional_files.iter().map(|e| e.path.as_bytes())) {
            Err(ValidateBuildRequestError::AdditionalFilesNotSorted)?;
        }

        Ok(Self {
            inputs,
            command_args: value.command_args,
            working_dir: PathBuf::from(value.working_dir),
            scratch_paths: value.scratch_paths.iter().map(PathBuf::from).collect(),
            inputs_dir: PathBuf::from(value.inputs_dir),
            outputs: value.outputs.iter().map(PathBuf::from).collect(),
            environment_vars: value.environment_vars.into_iter().map(Into::into).collect(),
            constraints,
            additional_files: value.additional_files.into_iter().map(Into::into).collect(),
            refscan_needles: value.refscan_needles,
        })
    }
}

/// Errors that occur during the validation of
/// [build_request::BuildConstraints] messages.
#[derive(Debug, thiserror::Error)]
pub enum ValidateBuildConstraintsError {
    #[error("invalid system")]
    InvalidSystem,

    #[error("invalid available_ro_paths at position {0}")]
    InvalidAvailableRoPaths(usize),

    #[error("available_ro_paths not sorted")]
    AvailableRoPathsNotSorted,
}

impl build_request::BuildConstraints {
    pub fn validate(&self) -> Result<(), ValidateBuildConstraintsError> {
        // validate system
        if self.system.is_empty() {
            Err(ValidateBuildConstraintsError::InvalidSystem)?;
        }
        // validate available_ro_paths
        for (i, p) in self.available_ro_paths.iter().enumerate() {
            if !is_clean_absolute_path(p) {
                Err(ValidateBuildConstraintsError::InvalidAvailableRoPaths(i))?
            }
        }
        if !is_sorted(self.available_ro_paths.iter().map(|e| e.as_bytes())) {
            Err(ValidateBuildConstraintsError::AvailableRoPathsNotSorted)?;
        }

        Ok(())
    }
}

impl From<build_request::EnvVar> for crate::buildservice::EnvVar {
    fn from(value: build_request::EnvVar) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<crate::buildservice::EnvVar> for build_request::EnvVar {
    fn from(value: crate::buildservice::EnvVar) -> Self {
        Self {
            key: value.key,
            value: value.value,
        }
    }
}

impl From<build_request::AdditionalFile> for crate::buildservice::AdditionalFile {
    fn from(value: build_request::AdditionalFile) -> Self {
        Self {
            path: PathBuf::from(value.path),
            contents: value.contents,
        }
    }
}

impl From<crate::buildservice::AdditionalFile> for build_request::AdditionalFile {
    fn from(value: crate::buildservice::AdditionalFile) -> Self {
        Self {
            path: value
                .path
                .to_str()
                .expect("Tvix bug: expected a valid path")
                .to_string(),
            contents: value.contents,
        }
    }
}

impl TryFrom<build_request::BuildConstraints> for HashSet<crate::buildservice::BuildConstraints> {
    type Error = ValidateBuildConstraintsError;
    fn try_from(value: build_request::BuildConstraints) -> Result<Self, Self::Error> {
        use crate::buildservice::BuildConstraints;

        // validate system
        if value.system.is_empty() {
            Err(ValidateBuildConstraintsError::InvalidSystem)?;
        }

        let mut build_constraints = HashSet::from([
            BuildConstraints::System(value.system),
            BuildConstraints::MinMemory(value.min_memory),
        ]);

        // validate available_ro_paths
        for (i, p) in value.available_ro_paths.iter().enumerate() {
            if !is_clean_absolute_path(p) {
                Err(ValidateBuildConstraintsError::InvalidAvailableRoPaths(i))?
            } else {
                build_constraints.insert(BuildConstraints::AvailableReadOnlyPath(PathBuf::from(p)));
            }
        }
        if !is_sorted(value.available_ro_paths.iter().map(|e| e.as_bytes())) {
            Err(ValidateBuildConstraintsError::AvailableRoPathsNotSorted)?;
        }

        if value.network_access {
            build_constraints.insert(BuildConstraints::NetworkAccess);
        }
        if value.provide_bin_sh {
            build_constraints.insert(BuildConstraints::ProvideBinSh);
        }

        Ok(build_constraints)
    }
}

#[cfg(test)]
// TODO: add testcases for constraints special cases. The default cases in the protos
// should result in the constraints not being added. For example min_memory 0 can be omitted.
// Also interesting testcases are "merging semantics". MimMemory(1) and MinMemory(100) will
// result in mim_memory 100, multiple AvailableReadOnlyPaths need to be merged. Contradicting
// system constraints need to fail somewhere (maybe an assertion, as only buggy code can construct it)
mod tests {
    use super::{is_clean_path, is_clean_relative_path};
    use rstest::rstest;

    #[rstest]
    #[case::fail_trailing_slash("foo/bar/", false)]
    #[case::fail_dotdot("foo/../bar", false)]
    #[case::fail_singledot("foo/./bar", false)]
    #[case::fail_unnecessary_slashes("foo//bar", false)]
    #[case::fail_absolute_unnecessary_slashes("//foo/bar", false)]
    #[case::ok_empty("", true)]
    #[case::ok_relative("foo/bar", true)]
    #[case::ok_absolute("/", true)]
    #[case::ok_absolute2("/foo/bar", true)]
    fn test_is_clean_path(#[case] s: &str, #[case] expected: bool) {
        assert_eq!(is_clean_path(s), expected);
    }

    #[rstest]
    #[case::fail_absolute("/", false)]
    #[case::ok_relative("foo/bar", true)]
    fn test_is_clean_relative_path(#[case] s: &str, #[case] expected: bool) {
        assert_eq!(is_clean_relative_path(s), expected);
    }

    // TODO: add tests for BuildRequest validation itself
}
