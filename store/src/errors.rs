use std::sync::PoisonError;
use thiserror::Error;
use tonic::Status;

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

impl From<Error> for Status {
    fn from(value: Error) -> Self {
        match value {
            Error::InvalidRequest(msg) => Status::invalid_argument(msg),
            Error::StorageError(msg) => Status::data_loss(format!("storage error: {}", msg)),
        }
    }
}

// TODO: this should probably go somewhere else?
impl From<Error> for std::io::Error {
    fn from(value: Error) -> Self {
        match value {
            Error::InvalidRequest(msg) => Self::new(std::io::ErrorKind::InvalidInput, msg),
            Error::StorageError(msg) => Self::new(std::io::ErrorKind::Other, msg),
        }
    }
}
