//! Contains errors that can occur during evaluation of builtins in this crate
use nix_compat::{
    nixhash::{self, NixHash},
    store_path::BuildStorePathError,
};
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

#[derive(Debug, Error)]
pub enum FetcherError {
    #[error("hash mismatch in file downloaded from {url}:\n  wanted: {wanted}\n     got: {got}")]
    HashMismatch {
        url: String,
        wanted: NixHash,
        got: NixHash,
    },

    #[error("Invalid hash type '{0}' for fetcher")]
    InvalidHashType(&'static str),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Import(#[from] tvix_castore::import::Error),

    #[error("Error calculating store path for fetcher output: {0}")]
    StorePath(#[from] BuildStorePathError),
}

/// Errors related to `builtins.path` and `builtins.filterSource`,
/// a.k.a. "importing" builtins.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("non-file '{0}' cannot be imported in 'flat' mode")]
    FlatImportOfNonFile(String),
    #[error("hash mismatch at ingestion of '{0}', expected: '{1}', got: '{2}'")]
    HashMismatch(String, NixHash, NixHash),
}

impl From<ImportError> for tvix_eval::ErrorKind {
    fn from(err: ImportError) -> Self {
        tvix_eval::ErrorKind::TvixError(Rc::new(err))
    }
}
