use crate::{nixbase32::Nixbase32DecodeError, store_path};
use thiserror::Error;

/// Errors that can occur during the validation of Derivation structs.
#[derive(Debug, Error, PartialEq)]
pub enum DerivationError {
    // outputs
    #[error("no outputs defined")]
    NoOutputs(),
    #[error("invalid output name: {0}")]
    InvalidOutputName(String),
    #[error("encountered fixed-output derivation, but more than 1 output in total")]
    MoreThanOneOutputButFixed(),
    #[error("invalid output name for fixed-output derivation: {0}")]
    InvalidOutputNameForFixed(String),
    #[error("unable to validate output {0}: {1}")]
    InvalidOutput(String, OutputError),
    // input derivation
    #[error("unable to parse input derivation path {0}: {1}")]
    InvalidInputDerivationPath(String, store_path::Error),
    #[error("input derivation {0} doesn't end with .drv")]
    InvalidInputDerivationPrefix(String),
    #[error("input derivation {0} output names are empty")]
    EmptyInputDerivationOutputNames(String),
    #[error("input derivation {0} output name {1} is invalid")]
    InvalidInputDerivationOutputName(String, String),

    // input sources
    #[error("unable to parse input sources path {0}: {1}")]
    InvalidInputSourcesPath(String, store_path::Error),

    // platform
    #[error("invalid platform field: {0}")]
    InvalidPlatform(String),

    // builder
    #[error("invalid builder field: {0}")]
    InvalidBuilder(String),

    // environment
    #[error("invalid environment key {0}")]
    InvalidEnvironmentKey(String),
}

/// Errors that can occur during the validation of a specific
// [crate::derivation::Output] of a [crate::derivation::Derviation].
#[derive(Debug, Error, PartialEq)]
pub enum OutputError {
    #[error("Invalid output path {0}: {1}")]
    InvalidOutputPath(String, store_path::Error),
    #[error("Invalid hash encoding: {0}")]
    InvalidHashEncoding(String, Nixbase32DecodeError),
    #[error("Invalid hash algo: {0}")]
    InvalidHashAlgo(String),
    #[error("Invalid Digest size {0} for algo {1}")]
    InvalidDigestSizeForAlgo(usize, String),
}
