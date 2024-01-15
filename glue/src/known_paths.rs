//! This module implements logic required for persisting known paths
//! during an evaluation.
//!
//! Tvix needs to be able to keep track of each Nix store path that it
//! knows about during the scope of a single evaluation and its
//! related builds.
//!
//! This data is required to find the derivation needed to actually trigger the
//! build, if necessary.

use nix_compat::nixhash::NixHash;
use std::collections::HashMap;

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
