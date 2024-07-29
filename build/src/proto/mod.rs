use std::path::{Path, PathBuf};

use itertools::Itertools;
use tvix_castore::directoryservice::NamedNode;
use tvix_castore::directoryservice::Node;
use tvix_castore::ValidateNodeError;

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
    InvalidInputNode(usize, ValidateNodeError),

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
        // now we can look at the names, and make sure they're sorted.
        if !is_sorted(
            self.inputs
                .iter()
                // TODO(flokli) handle conversion errors and store result somewhere
                .map(|e| {
                    Node::try_from(e.node.as_ref().unwrap())
                        .unwrap()
                        .get_name()
                        .clone()
                }),
        ) {
            Err(ValidateBuildRequestError::InputNodesNotSorted)?
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

#[cfg(test)]
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
