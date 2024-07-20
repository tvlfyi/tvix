use thiserror::Error;
use tokio::task::JoinError;
use tonic::Status;

/// Errors related to communication with the store.
#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("internal storage error: {0}")]
    StorageError(String),
}

impl From<JoinError> for Error {
    fn from(value: JoinError) -> Self {
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

impl From<crate::tonic::Error> for Error {
    fn from(value: crate::tonic::Error) -> Self {
        Self::StorageError(value.to_string())
    }
}

impl From<redb::Error> for Error {
    fn from(value: redb::Error) -> Self {
        Error::StorageError(value.to_string())
    }
}

impl From<redb::DatabaseError> for Error {
    fn from(value: redb::DatabaseError) -> Self {
        Error::StorageError(value.to_string())
    }
}

impl From<redb::TableError> for Error {
    fn from(value: redb::TableError) -> Self {
        Error::StorageError(value.to_string())
    }
}

impl From<redb::TransactionError> for Error {
    fn from(value: redb::TransactionError) -> Self {
        Error::StorageError(value.to_string())
    }
}

impl From<redb::StorageError> for Error {
    fn from(value: redb::StorageError) -> Self {
        Error::StorageError(value.to_string())
    }
}

impl From<redb::CommitError> for Error {
    fn from(value: redb::CommitError) -> Self {
        Error::StorageError(value.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        if value.kind() == std::io::ErrorKind::InvalidInput {
            Error::InvalidRequest(value.to_string())
        } else {
            Error::StorageError(value.to_string())
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
