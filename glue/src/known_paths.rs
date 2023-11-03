//! This module implements logic required for persisting known paths
//! during an evaluation.
//!
//! Tvix needs to be able to keep track of each Nix store path that it
//! knows about during the scope of a single evaluation and its
//! related builds.
//!
//! This data is required to scan derivation inputs for the build
//! references (the "build closure") that they make use of.
//!
//! Please see //tvix/eval/docs/build-references.md for more
//! information.

use crate::refscan::{ReferenceScanner, STORE_PATH_LEN};
use nix_compat::nixhash::NixHash;
use std::{
    collections::{hash_map, BTreeSet, HashMap},
    ops::Index,
};

#[derive(Debug, PartialEq)]
pub enum PathKind {
    /// A literal derivation (`.drv`-file), and the *names* of its outputs.
    Derivation { output_names: BTreeSet<String> },

    /// An output of a derivation, its name, and the path of its derivation.
    Output { name: String, derivation: String },

    /// A plain store path (e.g. source files copied to the store).
    Plain,
}

#[derive(Debug, PartialEq)]
pub struct KnownPath {
    pub path: String,
    pub kind: PathKind,
}

impl KnownPath {
    fn new(path: String, kind: PathKind) -> Self {
        KnownPath { path, kind }
    }
}

/// Internal struct to prevent accidental leaks of the truncated path
/// names.
#[repr(transparent)]
#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Ord, Eq, Hash)]
pub struct PathName(String);

impl From<&str> for PathName {
    fn from(s: &str) -> Self {
        PathName(s[..STORE_PATH_LEN].to_string())
    }
}

/// This instance is required to pass PathName instances as needles to
/// the reference scanner.
impl AsRef<[u8]> for PathName {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

#[derive(Debug, Default)]
pub struct KnownPaths {
    /// All known paths, keyed by a truncated version of their store
    /// path used for reference scanning.
    paths: HashMap<PathName, KnownPath>,

    /// All known derivation or FOD hashes.
    ///
    /// Keys are derivation paths, values is the NixHash.
    derivation_or_fod_hashes: HashMap<String, NixHash>,
}

impl Index<&PathName> for KnownPaths {
    type Output = KnownPath;

    fn index(&self, index: &PathName) -> &Self::Output {
        &self.paths[index]
    }
}

impl KnownPaths {
    fn insert_path(&mut self, path: String, path_kind: PathKind) {
        match self.paths.entry(path.as_str().into()) {
            hash_map::Entry::Vacant(entry) => {
                entry.insert(KnownPath::new(path, path_kind));
            }

            hash_map::Entry::Occupied(mut entry) => {
                match (path_kind, &mut entry.get_mut().kind) {
                    // These variant combinations require no "merging action".
                    (PathKind::Plain, PathKind::Plain) => (),
                    (PathKind::Output { .. }, PathKind::Output { .. }) => (),

                    (
                        PathKind::Derivation { output_names: new },
                        PathKind::Derivation {
                            output_names: ref mut old,
                        },
                    ) => {
                        old.extend(new);
                    }

                    _ => panic!(
                        "path '{}' inserted twice with different types",
                        entry.key().0
                    ),
                };
            }
        };
    }

    /// Mark a plain path as known.
    pub fn plain<S: ToString>(&mut self, path: S) {
        self.insert_path(path.to_string(), PathKind::Plain);
    }

    /// Mark a derivation as known.
    pub fn drv<P: ToString, O: ToString>(&mut self, path: P, outputs: &[O]) {
        self.insert_path(
            path.to_string(),
            PathKind::Derivation {
                output_names: outputs.iter().map(ToString::to_string).collect(),
            },
        );
    }

    /// Mark a derivation output path as known.
    pub fn output<P: ToString, N: ToString, D: ToString>(
        &mut self,
        output_path: P,
        name: N,
        drv_path: D,
    ) {
        self.insert_path(
            output_path.to_string(),
            PathKind::Output {
                name: name.to_string(),
                derivation: drv_path.to_string(),
            },
        );
    }

    /// Checks whether there are any known paths. If not, a reference
    /// scanner can not be created.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Create a reference scanner from the current set of known paths.
    pub fn reference_scanner(&self) -> ReferenceScanner<PathName> {
        let candidates = self.paths.keys().map(Clone::clone).collect();
        ReferenceScanner::new(candidates)
    }

    /// Fetch the opaque "hash derivation modulo" for a given derivation path.
    pub fn get_hash_derivation_modulo(&self, drv_path: &str) -> NixHash {
        // TODO: we rely on an invariant that things *should* have
        // been calculated if we get this far.
        self.derivation_or_fod_hashes[drv_path].clone()
    }

    pub fn add_hash_derivation_modulo<D: ToString>(
        &mut self,
        drv: D,
        hash_derivation_modulo: &NixHash,
    ) {
        #[allow(unused_variables)] // assertions on this only compiled in debug builds
        let old = self
            .derivation_or_fod_hashes
            .insert(drv.to_string(), hash_derivation_modulo.to_owned());

        #[cfg(debug_assertions)]
        {
            if let Some(old) = old {
                debug_assert!(
                    old == *hash_derivation_modulo,
                    "hash derivation modulo for a given derivation should always be calculated the same"
                );
            }
        }
    }
}
