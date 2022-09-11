//! Implements warnings that are emitted in cases where code passed to
//! Tvix exhibits problems that the user could address.

use codemap::CodeMap;
use codemap_diagnostic::{ColorConfig, Diagnostic, Emitter, Level, SpanLabel, SpanStyle};

#[derive(Debug)]
pub enum WarningKind {
    DeprecatedLiteralURL,
    UselessInherit,
    UnusedBinding,
    ShadowedGlobal(&'static str),

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
    pub fn fancy_format_str(&self, codemap: &CodeMap) -> String {
        let mut out = vec![];
        Emitter::vec(&mut out, Some(codemap)).emit(&[self.diagnostic(codemap)]);
        String::from_utf8_lossy(&out).to_string()
    }

    /// Render a fancy, human-readable output of this warning and
    /// print it to stderr. If rendered in a terminal that supports
    /// colours and font styles, the output will include those.
    pub fn fancy_format_stderr(&self, codemap: &CodeMap) {
        Emitter::stderr(ColorConfig::Auto, Some(codemap)).emit(&[self.diagnostic(codemap)]);
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
    fn message(&self, codemap: &CodeMap) -> String {
        match self.kind {
            WarningKind::DeprecatedLiteralURL => {
                format!("URL literal syntax is deprecated, use a quoted string instead")
            }

            WarningKind::UselessInherit => {
                format!("inherited variable already exists with the same value")
            }

            WarningKind::UnusedBinding => {
                let file = codemap.find_file(self.span.low());

                format!(
                    "variable '{}' is declared, but never used:",
                    file.source_slice(self.span)
                )
            }

            WarningKind::ShadowedGlobal(name) => {
                format!("declared variable '{}' shadows a built-in global!", name)
            }

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
            WarningKind::NotImplemented(_) => "W999",
        }
    }

    fn diagnostic(&self, codemap: &CodeMap) -> Diagnostic {
        let span_label = SpanLabel {
            label: self.span_label(),
            span: self.span,
            style: SpanStyle::Primary,
        };

        Diagnostic {
            level: Level::Warning,
            message: self.message(codemap),
            spans: vec![span_label],
            code: Some(self.code().into()),
        }
    }
}
