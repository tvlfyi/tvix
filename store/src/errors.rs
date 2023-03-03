use std::sync::PoisonError;
use thiserror::Error;

/// Errors related to communication with the store.
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("internal storage error: {0}")]
    StorageError(String),
}

impl<T> From<PoisonError<T>> for Error {
    fn from(value: PoisonError<T>) -> Self {
        Error::StorageError(value.to_string())
    }
}
