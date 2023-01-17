use thiserror::Error;
use tvix_store::store_path::ParseStorePathError;

/// Errors that can occur during the validation of Derivation structs.
#[derive(Debug, Error)]
pub enum DerivationError {
    // outputs
    #[error("No outputs defined.")]
    NoOutputs(),
    #[error("Invalid output name: {0}.")]
    InvalidOutputName(String),
    #[error("Encountered fixed-output derivation, but more than 1 output in total.")]
    MoreThanOneOutputButFixed(),
    #[error("Invalid output name for fixed-output derivation: {0}.")]
    InvalidOutputNameForFixed(String),
    #[error("Unable to validate output {0}: {1}.")]
    InvalidOutput(String, OutputError),
    // input derivation
    #[error("Unable to parse input derivation path {0}: {1}.")]
    InvalidInputDerivationPath(String, ParseStorePathError),
    #[error("Input Derivation {0} doesn't end with .drv.")]
    InvalidInputDerivationPrefix(String),
    #[error("Input Derivation {0} output names are empty.")]
    EmptyInputDerivationOutputNames(String),
    #[error("Input Derivation {0} output name {1} is invalid.")]
    InvalidInputDerivationOutputName(String, String),

    // input sources
    #[error("Unable to parse input sources path {0}: {1}.")]
    InvalidInputSourcesPath(String, ParseStorePathError),

    // platform
    #[error("Invalid platform field: {0}")]
    InvalidPlatform(String),

    // builder
    #[error("Invalid builder field: {0}")]
    InvalidBuilder(String),

    // environment
    #[error("Invalid environment key {0}")]
    InvalidEnvironmentKey(String),
}

/// Errors that can occur during the validation of a specific [Output] of a [Derviation].
#[derive(Debug, Error)]
pub enum OutputError {
    #[error("Invalid ouput path {0}: {1}")]
    InvalidOutputPath(String, ParseStorePathError),
    #[error("Invalid hash encoding: {0}")]
    InvalidHashEncoding(String, data_encoding::DecodeError),
    #[error("Invalid hash algo: {0}")]
    InvalidHashAlgo(String),
    #[error("Invalid Digest size {0} for algo {1}")]
    InvalidDigestSizeForAlgo(usize, String),
}
