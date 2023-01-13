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

use crate::refscan::ReferenceScanner;
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

pub struct KnownPaths {
    /// All known paths, and their associated [`PathType`].
    paths: HashMap<String, PathType>,
}

impl Index<&str> for KnownPaths {
    type Output = PathType;

    fn index(&self, index: &str) -> &Self::Output {
        &self.paths[index]
    }
}

impl KnownPaths {
    /// Mark a plain path as known.
    pub fn plain<S: ToString>(&mut self, path: S) {
        self.paths.insert(path.to_string(), PathType::Plain);
    }

    /// Mark a derivation as known.
    pub fn drv<P: ToString, O: ToString>(&mut self, path: P, outputs: &[O]) {
        match self.paths.entry(path.to_string()) {
            hash_map::Entry::Occupied(mut entry) => {
                for output in outputs {
                    match entry.get_mut() {
                        PathType::Derivation {
                            ref mut output_names,
                        } => {
                            output_names.insert(output.to_string());
                        }

                        // Branches like this explicitly panic right now to find odd
                        // situations where something unexpected is done with the
                        // same path being inserted twice as different types.
                        _ => panic!(
                            "bug: {} is already a known path, but not a derivation!",
                            path.to_string()
                        ),
                    }
                }
            }

            hash_map::Entry::Vacant(entry) => {
                let output_names = outputs.iter().map(|o| o.to_string()).collect();
                entry.insert(PathType::Derivation { output_names });
            }
        }
    }

    /// Mark a derivation output path as known.
    pub fn output<P: ToString, N: ToString, D: ToString>(
        &mut self,
        output_path: P,
        name: N,
        drv_path: D,
    ) {
        match self.paths.entry(output_path.to_string()) {
            hash_map::Entry::Occupied(entry) => {
                /* nothing to do, really! */
                debug_assert!(
                    *entry.get()
                        == PathType::Output {
                            name: name.to_string(),
                            derivation: drv_path.to_string(),
                        }
                );
            }

            hash_map::Entry::Vacant(entry) => {
                entry.insert(PathType::Output {
                    name: name.to_string(),
                    derivation: drv_path.to_string(),
                });
            }
        }
    }

    /// Create a reference scanner from the current set of known paths.
    pub fn reference_scanner(&self) -> ReferenceScanner {
        let candidates = self.paths.keys().map(Clone::clone).collect();
        ReferenceScanner::new(candidates)
    }
}
