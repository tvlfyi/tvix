use std::{fs::FileType, path::PathBuf};

use crate::{proto::ValidateDirectoryError, Error as CastoreError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to upload directory at {0}: {1}")]
    UploadDirectoryError(PathBuf, CastoreError),

    #[error("invalid encoding encountered for entry {0:?}")]
    InvalidEncoding(PathBuf),

    #[error("unable to stat {0}: {1}")]
    UnableToStat(PathBuf, std::io::Error),

    #[error("unable to open {0}: {1}")]
    UnableToOpen(PathBuf, std::io::Error),

    #[error("unable to read {0}: {1}")]
    UnableToRead(PathBuf, std::io::Error),

    #[error("error reading from archive: {0}")]
    Archive(std::io::Error),

    #[error("unsupported file {0} type: {1:?}")]
    UnsupportedFileType(PathBuf, FileType),

    #[error("invalid directory contents {0}: {1}")]
    InvalidDirectory(PathBuf, ValidateDirectoryError),

    #[error("unsupported tar entry {0} type: {1:?}")]
    UnsupportedTarEntry(PathBuf, tokio_tar::EntryType),
}

impl From<CastoreError> for Error {
    fn from(value: CastoreError) -> Self {
        match value {
            CastoreError::InvalidRequest(_) => panic!("tvix bug"),
            CastoreError::StorageError(_) => panic!("error"),
        }
    }
}

impl From<Error> for std::io::Error {
    fn from(value: Error) -> Self {
        std::io::Error::new(std::io::ErrorKind::Other, value)
    }
}
