use std::rc::Rc;
use thiserror::Error;
use tvix_derivation::DerivationError;

/// Errors related to derivation construction
#[derive(Debug, Error, PartialEq)]
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
    #[error("invalid output hash mode: '{0}', only 'recursive' and 'flat` are supported")]
    InvalidOutputHashMode(String),
    #[error("unsupported sri algorithm: {0}, only sha1, sha256 or sha512 is supported")]
    UnsupportedSRIAlgo(String),
    #[error("invalid number of sri hashes in string ({0}), only one hash is supported")]
    UnsupportedSRIMultiple(usize),
    #[error("invalid sri digest: {0}")]
    InvalidSRIDigest(data_encoding::DecodeError),
    #[error("failed to parse SRI string: {0}")]
    InvalidSRIString(String),
    #[error("outputHashAlgo is set to {0}, but outputHash contains SRI with algo {1}")]
    ConflictingSRIHashAlgo(String, String),
}

impl From<Error> for tvix_eval::ErrorKind {
    fn from(err: Error) -> Self {
        tvix_eval::ErrorKind::TvixError(Rc::new(err))
    }
}
