//! This module implements logic required for persisting known paths
//! during an evaluation.
//!
//! Tvix needs to be able to keep track of each Nix store path that it
//! knows about during the scope of a single evaluation and its
//! related builds.
//!
//! This data is required to find the derivation needed to actually trigger the
//! build, if necessary.

use nix_compat::{
    derivation::Derivation,
    nixhash::NixHash,
    store_path::{StorePath, StorePathRef},
};
use std::collections::HashMap;

/// Struct keeping track of all known Derivations in the current evaluation.
/// This keeps both the Derivation struct, as well as the "Hash derivation
/// modulo".
#[derive(Debug, Default)]
pub struct KnownPaths {
    /// All known derivation or FOD hashes.
    ///
    /// Keys are derivation paths, values are a tuple of the "hash derivation
    /// modulo" and the Derivation struct itself.
    derivations: HashMap<StorePath, (NixHash, Derivation)>,
}

impl KnownPaths {
    /// Fetch the opaque "hash derivation modulo" for a given derivation path.
    pub fn get_hash_derivation_modulo(&self, drv_path: &StorePathRef) -> Option<&NixHash> {
        self.derivations
            .get(&drv_path.to_owned())
            .map(|(hash_derivation_modulo, _derivation)| hash_derivation_modulo)
    }

    /// Return a reference to the Derivation for a given drv path.
    pub fn get_drv_by_drvpath(&self, drv_path: &StorePath) -> Option<&Derivation> {
        self.derivations
            .get(drv_path)
            .map(|(_hash_derivation_modulo, derivation)| derivation)
    }

    /// Insert a new Derivation into this struct.
    /// The Derivation struct must pass validation, and its output paths need to
    /// be fully calculated.
    /// All input derivations this refers to must also be inserted to this
    /// struct.
    pub fn add(&mut self, drv_path: StorePath, drv: Derivation) {
        // check input derivations to have been inserted.
        #[cfg(debug_assertions)]
        {
            // TODO: b/264
            // We assume derivations to be passed validated, so ignoring rest
            // and expecting parsing is ok.
            for input_drv_path_str in drv.input_derivations.keys() {
                let (input_drv_path, _rest) =
                    StorePath::from_absolute_path_full(input_drv_path_str)
                        .expect("parse input drv path");
                debug_assert!(self.derivations.contains_key(&input_drv_path));
            }
        }

        // compute the hash derivation modulo
        let hash_derivation_modulo = drv.derivation_or_fod_hash(|drv_path| {
            self.get_hash_derivation_modulo(drv_path)
                .unwrap_or_else(|| panic!("{} not found", drv_path))
                .to_owned()
        });

        #[allow(unused_variables)] // assertions on this only compiled in debug builds
        let old = self
            .derivations
            .insert(drv_path.to_owned(), (hash_derivation_modulo.clone(), drv));

        #[cfg(debug_assertions)]
        {
            if let Some(old) = old {
                debug_assert!(
                    old.0 == hash_derivation_modulo,
                    "hash derivation modulo for a given derivation should always be calculated the same"
                );
            }
        }
    }
}
