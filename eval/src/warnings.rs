//! Implements warnings that are emitted in cases where code passed to
//! Tvix exhibits problems that the user could address.

#[derive(Debug)]
pub enum WarningKind {
    DeprecatedLiteralURL,
    UselessInherit,
    UnusedBinding,
    ShadowedGlobal(&'static str),

    /// Tvix internal warning for features triggered by users that are
    /// not actually implemented yet.
    NotImplemented(&'static str),
}

#[derive(Debug)]
pub struct EvalWarning {
    pub kind: WarningKind,
    pub span: codemap::Span,
}
