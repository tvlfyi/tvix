use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::thread;

use bytes::Bytes;
use thiserror::Error;

use crate::nix_daemon::ProtocolVersion;

use super::NixRead;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum Error {
    #[error("custom error '{0}'")]
    Custom(String),
    #[error("invalid data '{0}'")]
    InvalidData(String),
    #[error("missing data '{0}'")]
    MissingData(String),
    #[error("IO error {0} '{1}'")]
    IO(io::ErrorKind, String),
    #[error("wrong read: expected {0} got {1}")]
    WrongRead(OperationType, OperationType),
}

impl Error {
    pub fn expected_read_number() -> Error {
        Error::WrongRead(OperationType::ReadNumber, OperationType::ReadBytes)
    }

    pub fn expected_read_bytes() -> Error {
        Error::WrongRead(OperationType::ReadBytes, OperationType::ReadNumber)
    }
}

impl super::Error for Error {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        Self::Custom(msg.to_string())
    }

    fn io_error(err: std::io::Error) -> Self {
        Self::IO(err.kind(), err.to_string())
    }

    fn invalid_data<T: fmt::Display>(msg: T) -> Self {
        Self::InvalidData(msg.to_string())
    }

    fn missing_data<T: fmt::Display>(msg: T) -> Self {
        Self::MissingData(msg.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationType {
    ReadNumber,
    ReadBytes,
}

impl fmt::Display for OperationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadNumber => write!(f, "read_number"),
            Self::ReadBytes => write!(f, "read_bytess"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Operation {
    ReadNumber(Result<u64, Error>),
    ReadBytes(Result<Bytes, Error>),
}

impl From<Operation> for OperationType {
    fn from(value: Operation) -> Self {
        match value {
            Operation::ReadNumber(_) => OperationType::ReadNumber,
            Operation::ReadBytes(_) => OperationType::ReadBytes,
        }
    }
}

pub struct Builder {
    version: ProtocolVersion,
    ops: VecDeque<Operation>,
}

impl Builder {
    pub fn new() -> Builder {
        Builder {
            version: Default::default(),
            ops: VecDeque::new(),
        }
    }

    pub fn version<V: Into<ProtocolVersion>>(&mut self, version: V) -> &mut Self {
        self.version = version.into();
        self
    }

    pub fn read_number(&mut self, value: u64) -> &mut Self {
        self.ops.push_back(Operation::ReadNumber(Ok(value)));
        self
    }

    pub fn read_number_error(&mut self, err: Error) -> &mut Self {
        self.ops.push_back(Operation::ReadNumber(Err(err)));
        self
    }

    pub fn read_bytes(&mut self, value: Bytes) -> &mut Self {
        self.ops.push_back(Operation::ReadBytes(Ok(value)));
        self
    }

    pub fn read_slice(&mut self, data: &[u8]) -> &mut Self {
        let value = Bytes::copy_from_slice(data);
        self.ops.push_back(Operation::ReadBytes(Ok(value)));
        self
    }

    pub fn read_bytes_error(&mut self, err: Error) -> &mut Self {
        self.ops.push_back(Operation::ReadBytes(Err(err)));
        self
    }

    pub fn build(&mut self) -> Mock {
        Mock {
            version: self.version,
            ops: self.ops.clone(),
        }
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Mock {
    version: ProtocolVersion,
    ops: VecDeque<Operation>,
}

impl NixRead for Mock {
    type Error = Error;

    fn version(&self) -> ProtocolVersion {
        self.version
    }

    async fn try_read_number(&mut self) -> Result<Option<u64>, Self::Error> {
        match self.ops.pop_front() {
            Some(Operation::ReadNumber(ret)) => ret.map(Some),
            Some(Operation::ReadBytes(_)) => Err(Error::expected_read_bytes()),
            None => Ok(None),
        }
    }

    async fn try_read_bytes_limited(
        &mut self,
        _limit: std::ops::RangeInclusive<usize>,
    ) -> Result<Option<Bytes>, Self::Error> {
        match self.ops.pop_front() {
            Some(Operation::ReadBytes(ret)) => ret.map(Some),
            Some(Operation::ReadNumber(_)) => Err(Error::expected_read_number()),
            None => Ok(None),
        }
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        // No need to panic again
        if thread::panicking() {
            return;
        }
        if let Some(op) = self.ops.front() {
            panic!("reader dropped with {op:?} operation still unread")
        }
    }
}

#[cfg(test)]
mod test {
    use bytes::Bytes;
    use hex_literal::hex;

    use crate::nix_daemon::de::NixRead;

    use super::{Builder, Error};

    #[tokio::test]
    async fn read_slice() {
        let mut mock = Builder::new()
            .read_number(10)
            .read_slice(&[])
            .read_slice(&hex!("0000 1234 5678 9ABC DEFF"))
            .build();
        assert_eq!(10, mock.read_number().await.unwrap());
        assert_eq!(&[] as &[u8], &mock.read_bytes().await.unwrap()[..]);
        assert_eq!(
            &hex!("0000 1234 5678 9ABC DEFF"),
            &mock.read_bytes().await.unwrap()[..]
        );
        assert_eq!(None, mock.try_read_number().await.unwrap());
        assert_eq!(None, mock.try_read_bytes().await.unwrap());
    }

    #[tokio::test]
    async fn read_bytes() {
        let mut mock = Builder::new()
            .read_number(10)
            .read_bytes(Bytes::from_static(&[]))
            .read_bytes(Bytes::from_static(&hex!("0000 1234 5678 9ABC DEFF")))
            .build();
        assert_eq!(10, mock.read_number().await.unwrap());
        assert_eq!(&[] as &[u8], &mock.read_bytes().await.unwrap()[..]);
        assert_eq!(
            &hex!("0000 1234 5678 9ABC DEFF"),
            &mock.read_bytes().await.unwrap()[..]
        );
        assert_eq!(None, mock.try_read_number().await.unwrap());
        assert_eq!(None, mock.try_read_bytes().await.unwrap());
    }

    #[tokio::test]
    async fn read_number() {
        let mut mock = Builder::new().read_number(10).build();
        assert_eq!(10, mock.read_number().await.unwrap());
        assert_eq!(None, mock.try_read_number().await.unwrap());
        assert_eq!(None, mock.try_read_bytes().await.unwrap());
    }

    #[tokio::test]
    async fn expect_number() {
        let mut mock = Builder::new().read_number(10).build();
        assert_eq!(
            Error::expected_read_number(),
            mock.read_bytes().await.unwrap_err()
        );
    }

    #[tokio::test]
    async fn expect_bytes() {
        let mut mock = Builder::new().read_slice(&[]).build();
        assert_eq!(
            Error::expected_read_bytes(),
            mock.read_number().await.unwrap_err()
        );
    }

    #[test]
    #[should_panic]
    fn operations_left() {
        let _ = Builder::new().read_number(10).build();
    }
}
