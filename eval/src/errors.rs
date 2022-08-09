use std::fmt::Display;

#[derive(Debug)]
pub enum Error {
    DuplicateAttrsKey {
        key: String,
    },

    TypeError {
        expected: &'static str,
        actual: &'static str,
    },
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{:?}", self)
    }
}

pub type EvalResult<T> = Result<T, Error>;
