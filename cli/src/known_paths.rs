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
use std::{
    collections::{hash_map, BTreeSet, HashMap},
    ops::Index,
};

#[derive(Debug, PartialEq)]
pub enum PathType {
    /// A literal derivation (`.drv`-file), and the *names* of its outputs.
    Derivation { output_names: BTreeSet<String> },

    /// An output of a derivation, its name, and the path of its derivation.
    Output { name: String, derivation: String },

    /// A plain store path (e.g. source files copied to the store).
    Plain,
}

#[derive(Debug, Default)]
pub struct KnownPaths {
    /// All known paths, and their associated [`PathType`].
    paths: HashMap<String, PathType>,

    /// All known replacement strings for derivations.
    ///
    /// Keys are derivation paths, values are the opaque replacement
    /// strings.
    replacements: HashMap<String, String>,
}

impl Index<&str> for KnownPaths {
    type Output = PathType;

    fn index(&self, index: &str) -> &Self::Output {
        &self.paths[&index[..STORE_PATH_LEN]]
    }
}

impl KnownPaths {
    fn insert_path(&mut self, path: String, path_type: PathType) {
        let path = path[..STORE_PATH_LEN].to_owned();
        assert_eq!(path.len(), STORE_PATH_LEN, "should match");
        match self.paths.entry(path) {
            hash_map::Entry::Vacant(entry) => {
                entry.insert(path_type);
            }

            hash_map::Entry::Occupied(mut entry) => {
                match (path_type, entry.get_mut()) {
                    // These variant combinations require no "merging action".
                    (PathType::Plain, PathType::Plain) => (),
                    (PathType::Output { .. }, PathType::Output { .. }) => (),

                    (
                        PathType::Derivation { output_names: new },
                        PathType::Derivation {
                            output_names: ref mut old,
                        },
                    ) => {
                        old.extend(new);
                    }

                    _ => panic!("path '{}' inserted twice with different types", entry.key()),
                };
            }
        };
    }

    /// Mark a plain path as known.
    pub fn plain<S: ToString>(&mut self, path: S) {
        self.insert_path(path.to_string(), PathType::Plain);
    }

    /// Mark a derivation as known.
    pub fn drv<P: ToString, O: ToString>(&mut self, path: P, outputs: &[O]) {
        self.insert_path(
            path.to_string(),
            PathType::Derivation {
                output_names: outputs.into_iter().map(ToString::to_string).collect(),
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
            PathType::Output {
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
    pub fn reference_scanner(&self) -> ReferenceScanner {
        let candidates = self.paths.keys().map(Clone::clone).collect();
        ReferenceScanner::new(candidates)
    }

    /// Fetch the opaque "replacement string" for a given derivation path.
    pub fn get_replacement_string(&self, drv: &str) -> String {
        // TODO: we rely on an invariant that things *should* have
        // been calculated if we get this far.
        self.replacements[drv].clone()
    }

    pub fn add_replacement_string<D: ToString>(&mut self, drv: D, replacement_str: &str) {
        let old = self
            .replacements
            .insert(drv.to_string(), replacement_str.to_owned());

        #[cfg(debug_assertions)]
        {
            if let Some(old) = old {
                debug_assert!(
                    old == replacement_str,
                    "replacement string for a given derivation should always match"
                );
            }
        }
    }
}
