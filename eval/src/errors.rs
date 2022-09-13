use crate::value::CoercionKind;
use std::fmt::Display;

use codemap::{CodeMap, Span};
use codemap_diagnostic::{Diagnostic, Emitter, Level, SpanLabel, SpanStyle};

use crate::Value;

#[derive(Clone, Debug)]
pub enum ErrorKind {
    /// These are user-generated errors through builtins.
    Throw(String),
    Abort(String),
    AssertionFailed,

    DuplicateAttrsKey {
        key: String,
    },

    /// Attempted to specify an invalid key type (e.g. integer) in a
    /// dynamic attribute name.
    InvalidAttributeName(Value),

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

    /// Resolving a user-supplied path literal failed in some way.
    PathResolution(String),

    /// Dynamic keys are not allowed in let.
    DynamicKeyInLet,

    /// Unknown variable in statically known scope.
    UnknownStaticVariable,

    /// Unknown variable in dynamic scope (with, rec, ...).
    UnknownDynamicVariable(String),

    /// User is defining the same variable twice at the same depth.
    VariableAlreadyDefined(Span),

    /// Attempt to call something that is not callable.
    NotCallable,

    /// Infinite recursion encountered while forcing thunks.
    InfiniteRecursion,

    ParseErrors(Vec<rnix::parser::ParseError>),

    /// An error occured while forcing a thunk, and needs to be
    /// chained up.
    ThunkForce(Box<Error>),

    /// Given type can't be coerced to a string in the respective context
    NotCoercibleToString {
        from: &'static str,
        kind: CoercionKind,
    },

    /// Tvix internal warning for features triggered by users that are
    /// not actually implemented yet, and without which eval can not
    /// proceed.
    NotImplemented(&'static str),
}

#[derive(Clone, Debug)]
pub struct Error {
    pub kind: ErrorKind,
    pub span: Span,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{:?}", self.kind)
    }
}

pub type EvalResult<T> = Result<T, Error>;

impl Error {
    pub fn fancy_format_str(&self, codemap: &CodeMap) -> String {
        let mut out = vec![];
        Emitter::vec(&mut out, Some(codemap)).emit(&[self.diagnostic(codemap)]);
        String::from_utf8_lossy(&out).to_string()
    }

    /// Create the optional span label displayed as an annotation on
    /// the underlined span of the error.
    fn span_label(&self) -> Option<String> {
        None
    }

    /// Create the primary error message displayed to users.
    fn message(&self, codemap: &CodeMap) -> String {
        match &self.kind {
            ErrorKind::Throw(msg) => format!("error thrown: {}", msg),
            ErrorKind::Abort(msg) => format!("evaluation aborted: {}", msg),
            ErrorKind::AssertionFailed => "assertion failed".to_string(),

            ErrorKind::DuplicateAttrsKey { key } => {
                format!("attribute key '{}' already defined", key)
            }

            ErrorKind::InvalidAttributeName(val) => format!(
                "found attribute name '{}' of type '{}', but attribute names must be strings",
                val,
                val.type_of()
            ),

            ErrorKind::AttributeNotFound { name } => format!(
                "attribute with name '{}' could not be found in the set",
                name
            ),

            ErrorKind::TypeError { expected, actual } => format!(
                "expected value of type '{}', but found a '{}'",
                expected, actual
            ),

            ErrorKind::Incomparable { lhs, rhs } => {
                format!("can not compare a {} with a {}", lhs, rhs)
            }

            ErrorKind::PathResolution(err) => format!("could not resolve path: {}", err),

            ErrorKind::DynamicKeyInLet => {
                "dynamically evaluated keys are not allowed in let-bindings".to_string()
            }

            ErrorKind::UnknownStaticVariable => "variable not found".to_string(),

            ErrorKind::UnknownDynamicVariable(name) => format!(
                r#"variable '{}' could not be found

Note that this occured within a `with`-expression. The problem may be related
to a missing value in the attribute set(s) included via `with`."#,
                name
            ),

            ErrorKind::VariableAlreadyDefined(_) => "variable has already been defined".to_string(),

            ErrorKind::NotCallable => {
                "this value is not callable (i.e. not a function or builtin)".to_string()
            }

            ErrorKind::InfiniteRecursion => "infinite recursion encountered".to_string(),

            // TODO(tazjin): these errors should actually end up with
            // individual spans etc.
            ErrorKind::ParseErrors(errors) => format!("failed to parse Nix code: {:?}", errors),

            // TODO(tazjin): trace through the whole chain of thunk
            // forcing errors with secondary spans, instead of just
            // delegating to the inner error
            ErrorKind::ThunkForce(err) => err.message(codemap),

            ErrorKind::NotCoercibleToString { kind, from } => {
                let kindly = match kind {
                    CoercionKind::Strong => "strongly",
                    CoercionKind::Weak => "weakly",
                };

                let hint = if *from == "set" {
                    ", missing a `__toString` or `outPath` attribute"
                } else {
                    ""
                };

                format!("cannot ({kindly}) coerce {from} to a string{hint}")
            }

            ErrorKind::NotImplemented(feature) => {
                format!("feature not yet implemented in Tvix: {}", feature)
            }
        }
    }

    /// Return the unique error code for this variant which can be
    /// used to refer users to documentation.
    fn code(&self) -> &'static str {
        match self.kind {
            ErrorKind::Throw(_) => "E001",
            ErrorKind::Abort(_) => "E002",
            ErrorKind::AssertionFailed => "E003",
            ErrorKind::InvalidAttributeName { .. } => "E004",
            ErrorKind::AttributeNotFound { .. } => "E005",
            ErrorKind::TypeError { .. } => "E006",
            ErrorKind::Incomparable { .. } => "E007",
            ErrorKind::PathResolution(_) => "E008",
            ErrorKind::DynamicKeyInLet => "E009",
            ErrorKind::UnknownStaticVariable => "E010",
            ErrorKind::UnknownDynamicVariable(_) => "E011",
            ErrorKind::VariableAlreadyDefined(_) => "E012",
            ErrorKind::NotCallable => "E013",
            ErrorKind::InfiniteRecursion => "E014",
            ErrorKind::ParseErrors(_) => "E015",
            ErrorKind::DuplicateAttrsKey { .. } => "E016",
            ErrorKind::ThunkForce(_) => "E017",
            ErrorKind::NotCoercibleToString { .. } => "E018",
            ErrorKind::NotImplemented(_) => "E999",
        }
    }

    fn diagnostic(&self, codemap: &CodeMap) -> Diagnostic {
        let span_label = SpanLabel {
            label: self.span_label(),
            span: self.span,
            style: SpanStyle::Primary,
        };

        Diagnostic {
            level: Level::Error,
            message: self.message(codemap),
            spans: vec![span_label],
            code: Some(self.code().into()),
        }
    }
}
