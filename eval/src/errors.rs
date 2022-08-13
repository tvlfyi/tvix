use std::fmt::Display;

#[derive(Debug)]
pub enum Error {
    DuplicateAttrsKey {
        key: String,
    },

    InvalidKeyType {
        given: &'static str,
    },

    AttributeNotFound {
        name: String,
    },

    TypeError {
        expected: &'static str,
        actual: &'static str,
    },

    Incomparable {
        lhs: &'static str,
        rhs: &'static str,
    },

    // Resolving a user-supplied path literal failed in some way.
    PathResolution(String),

    // Dynamic keys are not allowed in let.
    DynamicKeyInLet(rnix::SyntaxNode),
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{:?}", self)
    }
}

pub type EvalResult<T> = Result<T, Error>;
