//! Implements warnings that are emitted in cases where code passed to
//! Tvix exhibits problems that the user could address.

use codemap_diagnostic::{ColorConfig, Diagnostic, Emitter, Level, SpanLabel, SpanStyle};

use crate::SourceCode;

#[derive(Debug)]
pub enum WarningKind {
    DeprecatedLiteralURL,
    UselessInherit,
    UnusedBinding,
    ShadowedGlobal(&'static str),
    DeprecatedLegacyLet,
    InvalidNixPath(String),
    UselessBoolOperation(&'static str),
    DeadCode,
    EmptyInherit,
    EmptyLet,
    UselessParens,

    /// Tvix internal warning for features triggered by users that are
    /// not actually implemented yet, but do not cause runtime failures.
    NotImplemented(&'static str),
}

#[derive(Debug)]
pub struct EvalWarning {
    pub kind: WarningKind,
    pub span: codemap::Span,
}

impl EvalWarning {
    /// Render a fancy, human-readable output of this warning and
    /// return it as a String. Note that this version of the output
    /// does not include any colours or font styles.
    pub fn fancy_format_str(&self, source: &SourceCode) -> String {
        let mut out = vec![];
        Emitter::vec(&mut out, Some(&*source.codemap())).emit(&[self.diagnostic(source)]);
        String::from_utf8_lossy(&out).to_string()
    }

    /// Render a fancy, human-readable output of this warning and
    /// print it to stderr. If rendered in a terminal that supports
    /// colours and font styles, the output will include those.
    pub fn fancy_format_stderr(&self, source: &SourceCode) {
        Emitter::stderr(ColorConfig::Auto, Some(&*source.codemap()))
            .emit(&[self.diagnostic(source)]);
    }

    /// Create the optional span label displayed as an annotation on
    /// the underlined span of the warning.
    fn span_label(&self) -> Option<String> {
        match self.kind {
            WarningKind::UnusedBinding | WarningKind::ShadowedGlobal(_) => {
                Some("variable declared here".into())
            }
            _ => None,
        }
    }

    /// Create the primary warning message displayed to users for a
    /// warning.
    fn message(&self, source: &SourceCode) -> String {
        match self.kind {
            WarningKind::DeprecatedLiteralURL => {
                "URL literal syntax is deprecated, use a quoted string instead".to_string()
            }

            WarningKind::UselessInherit => {
                "inherit does nothing (this variable already exists with the same value)"
                    .to_string()
            }

            WarningKind::UnusedBinding => {
                format!(
                    "variable '{}' is declared, but never used:",
                    source.source_slice(self.span)
                )
            }

            WarningKind::ShadowedGlobal(name) => {
                format!("declared variable '{}' shadows a built-in global!", name)
            }

            WarningKind::DeprecatedLegacyLet => {
                "legacy `let` syntax used, please rewrite this as `let .. in ...`".to_string()
            }

            WarningKind::InvalidNixPath(ref err) => {
                format!("invalid NIX_PATH resulted in a parse error: {}", err)
            }

            WarningKind::UselessBoolOperation(msg) => {
                format!("useless operation on boolean: {}", msg)
            }

            WarningKind::DeadCode => "this code will never be executed".to_string(),

            WarningKind::EmptyInherit => "this `inherit` statement is empty".to_string(),

            WarningKind::EmptyLet => "this `let`-expression contains no bindings".to_string(),

            WarningKind::UselessParens => "these parenthesis can be removed".to_string(),

            WarningKind::NotImplemented(what) => {
                format!("feature not yet implemented in tvix: {}", what)
            }
        }
    }

    /// Return the unique warning code for this variant which can be
    /// used to refer users to documentation.
    fn code(&self) -> &'static str {
        match self.kind {
            WarningKind::DeprecatedLiteralURL => "W001",
            WarningKind::UselessInherit => "W002",
            WarningKind::UnusedBinding => "W003",
            WarningKind::ShadowedGlobal(_) => "W004",
            WarningKind::DeprecatedLegacyLet => "W005",
            WarningKind::InvalidNixPath(_) => "W006",
            WarningKind::UselessBoolOperation(_) => "W007",
            WarningKind::DeadCode => "W008",
            WarningKind::EmptyInherit => "W009",
            WarningKind::EmptyLet => "W010",
            WarningKind::UselessParens => "W011",

            WarningKind::NotImplemented(_) => "W999",
        }
    }

    fn diagnostic(&self, source: &SourceCode) -> Diagnostic {
        let span_label = SpanLabel {
            label: self.span_label(),
            span: self.span,
            style: SpanStyle::Primary,
        };

        Diagnostic {
            level: Level::Warning,
            message: self.message(source),
            spans: vec![span_label],
            code: Some(self.code().into()),
        }
    }
}
