//! This module implements logic required for persisting known paths
//! during an evaluation.
//!
//! Tvix needs to be able to keep track of each Nix store path that it
//! knows about during the scope of a single evaluation and its
//! related builds.
//!
//! This data is required to find the derivation needed to actually trigger the
//! build, if necessary.

use nix_compat::{nixhash::NixHash, store_path::StorePath};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct KnownPaths {
    /// All known derivation or FOD hashes.
    ///
    /// Keys are derivation paths, values is the NixHash.
    derivation_or_fod_hashes: HashMap<StorePath, NixHash>,
}

impl KnownPaths {
    /// Fetch the opaque "hash derivation modulo" for a given derivation path.
    pub fn get_hash_derivation_modulo(&self, drv_path: &StorePath) -> NixHash {
        // TODO: we rely on an invariant that things *should* have
        // been calculated if we get this far.
        self.derivation_or_fod_hashes[drv_path].clone()
    }

    pub fn add_hash_derivation_modulo(
        &mut self,
        drv_path: StorePath,
        hash_derivation_modulo: &NixHash,
    ) {
        #[allow(unused_variables)] // assertions on this only compiled in debug builds
        let old = self
            .derivation_or_fod_hashes
            .insert(drv_path, hash_derivation_modulo.to_owned());

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
