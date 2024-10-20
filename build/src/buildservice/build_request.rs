use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use bytes::Bytes;
use tvix_castore::{Node, PathComponent};
/// A BuildRequest describes the request of something to be run on the builder.
/// It is distinct from an actual \[Build\] that has already happened, or might be
/// currently ongoing.
///
/// A BuildRequest can be seen as a more normalized version of a Derivation
/// (parsed from A-Term), "writing out" some of the Nix-internal details about
/// how e.g. environment variables in the build are set.
///
/// Nix has some impurities when building a Derivation, for example the --cores option
/// ends up as an environment variable in the build, that's not part of the ATerm.
///
/// As of now, we serialize this into the BuildRequest, so builders can stay dumb.
/// This might change in the future.
///
/// There's also a big difference when it comes to how inputs are modelled:
///
/// * Nix only uses store path (strings) to describe the inputs.
///   As store paths can be input-addressed, a certain store path can contain
///   different contents (as not all store paths are binary reproducible).
///   This requires that for every input-addressed input, the builder has access
///   to either the input's deriver (and needs to build it) or else a trusted
///   source for the built input.
///   to upload input-addressed paths, requiring the trusted users concept.
/// * tvix-build records a list of tvix.castore.v1.Node as inputs.
///   These map from the store path base name to their contents, relieving the
///   builder from having to "trust" any input-addressed paths, contrary to Nix.
///
/// While this approach gives a better hermeticity, it has one downside:
/// A BuildRequest can only be sent once the contents of all its inputs are known.
///
/// As of now, we're okay to accept this, but it prevents uploading an
/// entirely-non-IFD subgraph of BuildRequests eagerly.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct BuildRequest {
    /// The list of all root nodes that should be visible in `inputs_dir` at the
    /// time of the build.
    /// As all references are content-addressed, no additional signatures are
    /// needed to substitute / make these available in the build environment.
    pub inputs: BTreeMap<PathComponent, Node>,
    /// The command (and its args) executed as the build script.
    /// In the case of a Nix derivation, this is usually
    /// \["/path/to/some-bash/bin/bash", "-e", "/path/to/some/builder.sh"\].
    pub command_args: Vec<String>,
    /// The working dir of the command, relative to the build root.
    /// "build", in the case of Nix.
    /// This MUST be a clean relative path, without any ".", "..", or superfluous
    /// slashes.
    pub working_dir: PathBuf,
    /// A list of "scratch" paths, relative to the build root.
    /// These will be write-able during the build.
    /// \[build, nix/store\] in the case of Nix.
    /// These MUST be clean relative paths, without any ".", "..", or superfluous
    /// slashes, and sorted.
    pub scratch_paths: Vec<PathBuf>,
    /// The path where the castore input nodes will be located at,
    /// "nix/store" in case of Nix.
    /// Builds might also write into here (Nix builds do that).
    /// This MUST be a clean relative path, without any ".", "..", or superfluous
    /// slashes.
    pub inputs_dir: PathBuf,
    /// The list of output paths the build is expected to produce,
    /// relative to the root.
    /// If the path is not produced, the build is considered to have failed.
    /// These MUST be clean relative paths, without any ".", "..", or superfluous
    /// slashes, and sorted.
    pub outputs: Vec<PathBuf>,
    /// The list of environment variables and their values that should be set
    /// inside the build environment.
    /// This includes both environment vars set inside the derivation, as well as
    /// more "ephemeral" ones like NIX_BUILD_CORES, controlled by the `--cores`
    /// CLI option of `nix-build`.
    /// For now, we consume this as an option when turning a Derivation into a BuildRequest,
    /// similar to how Nix has a `--cores` option.
    /// We don't want to bleed these very nix-specific sandbox impl details into
    /// (dumber) builders if we don't have to.
    /// Environment variables are sorted by their keys.
    pub environment_vars: Vec<EnvVar>,
    /// A set of constraints that need to be satisfied on a build host before a
    /// Build can be started.
    pub constraints: HashSet<BuildConstraints>,
    /// Additional (small) files and their contents that should be placed into the
    /// build environment, but outside inputs_dir.
    /// Used for passAsFile and structuredAttrs in Nix.
    pub additional_files: Vec<AdditionalFile>,
    /// If this is an non-empty list, all paths in `outputs` are scanned for these.
    /// For Nix, `refscan_needles` would be populated with the nixbase32 hash parts of
    /// every input store path and output store path. The latter is necessary to scan
    /// for references between multi-output derivations.
    pub refscan_needles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnvVar {
    /// name of the environment variable. Must not contain =.
    pub key: String,
    pub value: Bytes,
}
/// BuildConstraints represents certain conditions that must be fulfilled
/// inside the build environment to be able to build this.
/// Constraints can be things like required architecture and minimum amount of memory.
/// The required input paths are *not* represented in here, because it
/// wouldn't be hermetic enough - see the comment around inputs too.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BuildConstraints {
    /// The system that's needed to execute the build.
    /// Must not be empty.
    System(String),
    /// The amount of memory required to be available for the build, in bytes.
    MinMemory(u64),
    /// An absolute path that need to be available in the build
    /// environment, like `/dev/kvm`.
    /// This is distinct from the castore nodes in inputs.
    /// These MUST be clean absolute paths, without any ".", "..", or superfluous
    /// slashes, and sorted.
    AvailableReadOnlyPath(PathBuf),
    /// Whether the build should be able to access the network.
    NetworkAccess,
    /// Whether to provide a /bin/sh inside the build environment, usually a static bash.
    ProvideBinSh,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdditionalFile {
    pub path: PathBuf,
    pub contents: Bytes,
}
