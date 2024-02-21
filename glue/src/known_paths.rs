//! This module implements logic required for persisting known paths
//! during an evaluation.
//!
//! Tvix needs to be able to keep track of each Nix store path that it
//! knows about during the scope of a single evaluation and its
//! related builds.
//!
//! This data is required to find the derivation needed to actually trigger the
//! build, if necessary.

use nix_compat::{derivation::Derivation, nixhash::NixHash, store_path::StorePath};
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

    /// A map from output path to (one) drv path.
    /// Note that in the case of FODs, multiple drvs can produce the same output
    /// path. We use one of them.
    outputs_to_drvpath: HashMap<StorePath, StorePath>,
}

impl KnownPaths {
    /// Fetch the opaque "hash derivation modulo" for a given derivation path.
    pub fn get_hash_derivation_modulo(&self, drv_path: &StorePath) -> Option<&NixHash> {
        self.derivations
            .get(drv_path)
            .map(|(hash_derivation_modulo, _derivation)| hash_derivation_modulo)
    }

    /// Return a reference to the Derivation for a given drv path.
    pub fn get_drv_by_drvpath(&self, drv_path: &StorePath) -> Option<&Derivation> {
        self.derivations
            .get(drv_path)
            .map(|(_hash_derivation_modulo, derivation)| derivation)
    }

    /// Return the drv path of the derivation producing the passed output path.
    /// Note there can be multiple Derivations producing the same output path in
    /// flight; this function will only return one of them.
    pub fn get_drv_path_for_output_path(&self, output_path: &StorePath) -> Option<&StorePath> {
        self.outputs_to_drvpath.get(output_path)
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
            for input_drv_path in drv.input_derivations.keys() {
                debug_assert!(self.derivations.contains_key(input_drv_path));
            }
        }

        // compute the hash derivation modulo
        let hash_derivation_modulo = drv.derivation_or_fod_hash(|drv_path| {
            self.get_hash_derivation_modulo(&drv_path.to_owned())
                .unwrap_or_else(|| panic!("{} not found", drv_path))
                .to_owned()
        });

        // For all output paths, update our lookup table.
        // We only write into the lookup table once.
        for output in drv.outputs.values() {
            self.outputs_to_drvpath
                .entry(output.path.as_ref().expect("missing store path").clone())
                .or_insert(drv_path.to_owned());
        }

        // insert the derivation itself
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

#[cfg(test)]
mod tests {
    use nix_compat::{derivation::Derivation, nixhash::NixHash, store_path::StorePath};

    use super::KnownPaths;
    use hex_literal::hex;
    use lazy_static::lazy_static;

    lazy_static! {
        static ref BAR_DRV: Derivation = Derivation::from_aterm_bytes(include_bytes!(
            "tests/ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv"
        ))
        .expect("must parse");
        static ref FOO_DRV: Derivation = Derivation::from_aterm_bytes(include_bytes!(
            "tests/ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv"
        ))
        .expect("must parse");
        static ref BAR_DRV_PATH: StorePath =
            StorePath::from_bytes(b"ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv").expect("must parse");
        static ref FOO_DRV_PATH: StorePath =
            StorePath::from_bytes(b"ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv").expect("must parse");
        static ref BAR_OUT_PATH: StorePath =
            StorePath::from_bytes(b"mp57d33657rf34lzvlbpfa1gjfv5gmpg-bar").expect("must parse");
        static ref FOO_OUT_PATH: StorePath =
            StorePath::from_bytes(b"fhaj6gmwns62s6ypkcldbaj2ybvkhx3p-foo").expect("must parse");
    }

    /// ensure we don't allow acdding a Derivation that depends on another,
    /// not-yet-added Derivation.
    #[test]
    #[should_panic]
    fn reject_if_missing_input_drv() {
        let mut known_paths = KnownPaths::default();

        // FOO_DRV depends on BAR_DRV, which wasn't added.
        known_paths.add(FOO_DRV_PATH.clone(), FOO_DRV.clone());
    }

    #[test]
    fn happy_path() {
        let mut known_paths = KnownPaths::default();

        // get_drv_by_drvpath should return None for non-existing Derivations,
        // same as get_hash_derivation_modulo and get_drv_path_for_output_path
        assert_eq!(None, known_paths.get_drv_by_drvpath(&BAR_DRV_PATH));
        assert_eq!(None, known_paths.get_hash_derivation_modulo(&BAR_DRV_PATH));
        assert_eq!(
            None,
            known_paths.get_drv_path_for_output_path(&BAR_OUT_PATH)
        );

        // Add BAR_DRV
        known_paths.add(BAR_DRV_PATH.clone(), BAR_DRV.clone());

        // We should get it back
        assert_eq!(
            Some(&BAR_DRV.clone()),
            known_paths.get_drv_by_drvpath(&BAR_DRV_PATH)
        );

        // Test get_drv_path_for_output_path
        assert_eq!(
            Some(&BAR_DRV_PATH.clone()),
            known_paths.get_drv_path_for_output_path(&BAR_OUT_PATH)
        );

        // It should be possible to get the hash derivation modulo.
        assert_eq!(
            Some(&NixHash::Sha256(hex!(
                "c79aebd0ce3269393d4a1fde2cbd1d975d879b40f0bf40a48f550edc107fd5df"
            ))),
            known_paths.get_hash_derivation_modulo(&BAR_DRV_PATH.clone())
        );

        // Now insert FOO_DRV too. It shouldn't panic, as BAR_DRV is already
        // added.
        known_paths.add(FOO_DRV_PATH.clone(), FOO_DRV.clone());

        assert_eq!(
            Some(&FOO_DRV.clone()),
            known_paths.get_drv_by_drvpath(&FOO_DRV_PATH)
        );
        assert_eq!(
            Some(&NixHash::Sha256(hex!(
                "af030d36d63d3d7f56a71adaba26b36f5fa1f9847da5eed953ed62e18192762f"
            ))),
            known_paths.get_hash_derivation_modulo(&FOO_DRV_PATH.clone())
        );

        // Test get_drv_path_for_output_path
        assert_eq!(
            Some(&FOO_DRV_PATH.clone()),
            known_paths.get_drv_path_for_output_path(&FOO_OUT_PATH)
        );
    }
}
