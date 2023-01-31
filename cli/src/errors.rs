use nix_compat::{derivation::DerivationError, nixhash};
use std::rc::Rc;
use thiserror::Error;

/// Errors related to derivation construction
#[derive(Debug, Error)]
pub enum Error {
    #[error("an output with the name '{0}' is already defined")]
    DuplicateOutput(String),
    #[error("fixed-output derivations can only have the default `out`-output")]
    ConflictingOutputTypes,
    #[error("the environment variable '{0}' has already been set in this derivation")]
    DuplicateEnvVar(String),
    #[error("the environment variable '{0}' shadows the name of an output")]
    ShadowedOutput(String),
    #[error("invalid derivation parameters: {0}")]
    InvalidDerivation(DerivationError),
    #[error("invalid output hash: {0}")]
    InvalidOutputHash(nixhash::Error),
    #[error("invalid output hash mode: '{0}', only 'recursive' and 'flat` are supported")]
    InvalidOutputHashMode(String),
}

impl From<Error> for tvix_eval::ErrorKind {
    fn from(err: Error) -> Self {
        tvix_eval::ErrorKind::TvixError(Rc::new(err))
    }
}
