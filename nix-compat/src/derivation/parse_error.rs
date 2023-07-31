//! This contains error and result types that can happen while parsing
//! Derivations from ATerm.
use nom::IResult;

use crate::nixhash;

pub type NomResult<I, O> = IResult<I, O, NomError<I>>;

#[derive(Debug, PartialEq)]
pub enum ErrorKind {
    // duplicate key in map
    DuplicateMapKey(String),

    // Digest parsing error
    NixHashError(nixhash::Error),

    // error kind wrapped from native nom errors
    Nom(nom::error::ErrorKind),
}

/// Our own error type to pass along parser-related errors.
#[derive(Debug, PartialEq)]
pub struct NomError<I> {
    /// position of the error in the input data
    pub input: I,
    /// error code
    pub code: ErrorKind,
}

impl<I, E> nom::error::FromExternalError<I, E> for NomError<I> {
    fn from_external_error(input: I, kind: nom::error::ErrorKind, _e: E) -> Self {
        Self {
            input,
            code: ErrorKind::Nom(kind),
        }
    }
}

impl<I> nom::error::ParseError<I> for NomError<I> {
    fn from_error_kind(input: I, kind: nom::error::ErrorKind) -> Self {
        Self {
            input,
            code: ErrorKind::Nom(kind),
        }
    }

    // FUTUREWORK: implement, so we have support for backtracking through the
    // parse tree?
    fn append(_input: I, _kind: nom::error::ErrorKind, other: Self) -> Self {
        other
    }
}

/// This wraps a [nom::error::Error] into our error.
impl<I> From<nom::error::Error<I>> for NomError<I> {
    fn from(value: nom::error::Error<I>) -> Self {
        Self {
            input: value.input,
            code: ErrorKind::Nom(value.code),
        }
    }
}

/// This essentially implements
/// From<nom::Err<nom::error::Error<I>>> for nom::Err<NomError<I>>,
/// which we can't because nom::Err<_> is a foreign type.
pub(crate) fn into_nomerror<I>(e: nom::Err<nom::error::Error<I>>) -> nom::Err<NomError<I>> {
    match e {
        nom::Err::Incomplete(n) => nom::Err::Incomplete(n),
        nom::Err::Error(e) => nom::Err::Error(e.into()),
        nom::Err::Failure(e) => nom::Err::Failure(e.into()),
    }
}
