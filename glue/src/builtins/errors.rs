//! Contains errors that can occur during evaluation of builtins in this crate
use nix_compat::nixhash;
use std::rc::Rc;
use thiserror::Error;

/// Errors related to derivation construction
#[derive(Debug, Error)]
pub enum DerivationError {
    #[error("an output with the name '{0}' is already defined")]
    DuplicateOutput(String),
    #[error("fixed-output derivations can only have the default `out`-output")]
    ConflictingOutputTypes,
    #[error("the environment variable '{0}' has already been set in this derivation")]
    DuplicateEnvVar(String),
    #[error("invalid derivation parameters: {0}")]
    InvalidDerivation(#[from] nix_compat::derivation::DerivationError),
    #[error("invalid output hash: {0}")]
    InvalidOutputHash(#[from] nixhash::Error),
    #[error("invalid output hash mode: '{0}', only 'recursive' and 'flat` are supported")]
    InvalidOutputHashMode(String),
}

impl From<DerivationError> for tvix_eval::ErrorKind {
    fn from(err: DerivationError) -> Self {
        tvix_eval::ErrorKind::TvixError(Rc::new(err))
    }
}
