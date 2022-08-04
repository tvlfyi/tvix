use std::fmt::Display;

#[derive(Debug)]
pub struct Error {}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "error")
    }
}

pub type EvalResult<T> = Result<T, Error>;
