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

use crate::refscan::STORE_PATH_LEN;
use nix_compat::nixhash::NixHash;
use std::collections::{BTreeSet, HashMap};

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

impl From<String> for PathName {
    fn from(s: String) -> Self {
        s.as_str().into()
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
    /// All known derivation or FOD hashes.
    ///
    /// Keys are derivation paths, values is the NixHash.
    derivation_or_fod_hashes: HashMap<String, NixHash>,
}

impl KnownPaths {
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
