use bstr::ByteSlice;
use thiserror::Error;
use tokio::task::JoinError;
use tonic::Status;

use crate::{
    path::{PathComponent, PathComponentError},
    SymlinkTargetError,
};

/// Errors related to communication with the store.
#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("internal storage error: {0}")]
    StorageError(String),
}

/// Errors that occur during construction of [crate::Node]
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ValidateNodeError {
    /// Invalid digest length encountered
    #[error("invalid digest length: {0}")]
    InvalidDigestLen(usize),
    /// Invalid symlink target
    #[error("Invalid symlink target: {0}")]
    InvalidSymlinkTarget(SymlinkTargetError),
}

impl From<crate::digests::Error> for ValidateNodeError {
    fn from(e: crate::digests::Error) -> Self {
        match e {
            crate::digests::Error::InvalidDigestLen(n) => ValidateNodeError::InvalidDigestLen(n),
        }
    }
}

/// Errors that can occur when populating [crate::Directory] messages,
/// or parsing [crate::proto::Directory]
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DirectoryError {
    /// Multiple elements with the same name encountered
    #[error("{:?} is a duplicate name", .0)]
    DuplicateName(PathComponent),
    /// Node failed validation
    #[error("invalid node with name {}: {:?}", .0, .1.to_string())]
    InvalidNode(PathComponent, ValidateNodeError),
    #[error("Total size exceeds u64::MAX")]
    SizeOverflow,
    /// Invalid name encountered
    #[error("Invalid name: {0}")]
    InvalidName(PathComponentError),
    /// Elements are not in sorted order. Can only happen on protos
    #[error("{:?} is not sorted", .0.as_bstr())]
    WrongSorting(bytes::Bytes),
    /// This can only happen if there's an unknown node type (on protos)
    #[error("No node set")]
    NoNodeSet,
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
