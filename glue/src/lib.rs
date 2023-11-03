use std::{cell::RefCell, rc::Rc};

use known_paths::KnownPaths;

pub mod derivation;
pub mod errors;
pub mod known_paths;
pub mod refscan;
pub mod tvix_io;
pub mod tvix_store_io;

/// Adds derivation-related builtins to the passed [tvix_eval::Evaluation].
///
/// These are `derivation` and `derivationStrict`.
///
/// As they need to interact with `known_paths`, we also need to pass in
/// `known_paths`.
pub fn add_derivation_builtins(
    eval: &mut tvix_eval::Evaluation,
    known_paths: Rc<RefCell<KnownPaths>>,
) {
    eval.builtins
        .extend(derivation::derivation_builtins(known_paths));

    // Add the actual `builtins.derivation` from compiled Nix code
    eval.src_builtins
        .push(("derivation", include_str!("derivation.nix")));
}
