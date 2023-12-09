//! Utilities for dealing with span tracking in the compiler and in
//! error reporting.

use codemap::{File, Span};
use rnix::ast;
use rowan::ast::AstNode;

/// Helper struct to carry information required for making a span, but
/// without actually performing the (expensive) span lookup.
///
/// This is used for tracking spans across thunk boundaries, as they
/// are frequently instantiated but spans are only used in error or
/// warning cases.
#[derive(Clone, Debug)]
pub enum LightSpan {
    /// The span has already been computed and can just be used right
    /// away.
    Actual { span: Span },
}

impl LightSpan {
    pub fn new_actual(span: Span) -> Self {
        Self::Actual { span }
    }

    pub fn span(&self) -> Span {
        match self {
            LightSpan::Actual { span } => *span,
        }
    }
}

impl From<Span> for LightSpan {
    fn from(span: Span) -> Self {
        LightSpan::Actual { span }
    }
}

/// Trait implemented by all types from which we can retrieve a span.
pub trait ToSpan {
    fn span_for(&self, file: &File) -> Span;
}

impl ToSpan for Span {
    fn span_for(&self, _: &File) -> Span {
        *self
    }
}

impl ToSpan for rnix::TextRange {
    fn span_for(&self, file: &File) -> Span {
        file.span
            .subspan(u32::from(self.start()) as u64, u32::from(self.end()) as u64)
    }
}

impl ToSpan for rnix::SyntaxToken {
    fn span_for(&self, file: &File) -> Span {
        self.text_range().span_for(file)
    }
}

impl ToSpan for rnix::SyntaxNode {
    fn span_for(&self, file: &File) -> Span {
        self.text_range().span_for(file)
    }
}

/// Generates a `ToSpan` implementation for a type implementing
/// `rowan::AstNode`. This is impossible to do as a blanket
/// implementation because `rustc` forbids these implementations for
/// traits from third-party crates due to a belief that semantic
/// versioning truly could work (it doesn't).
macro_rules! expr_to_span {
    ( $type:path ) => {
        impl ToSpan for $type {
            fn span_for(&self, file: &File) -> Span {
                self.syntax().span_for(file)
            }
        }
    };
}

expr_to_span!(ast::Expr);
expr_to_span!(ast::Apply);
expr_to_span!(ast::Assert);
expr_to_span!(ast::Attr);
expr_to_span!(ast::AttrSet);
expr_to_span!(ast::Attrpath);
expr_to_span!(ast::AttrpathValue);
expr_to_span!(ast::BinOp);
expr_to_span!(ast::HasAttr);
expr_to_span!(ast::Ident);
expr_to_span!(ast::IdentParam);
expr_to_span!(ast::IfElse);
expr_to_span!(ast::Inherit);
expr_to_span!(ast::Interpol);
expr_to_span!(ast::Lambda);
expr_to_span!(ast::LegacyLet);
expr_to_span!(ast::LetIn);
expr_to_span!(ast::List);
expr_to_span!(ast::Literal);
expr_to_span!(ast::PatBind);
expr_to_span!(ast::Path);
expr_to_span!(ast::Pattern);
expr_to_span!(ast::Select);
expr_to_span!(ast::Str);
expr_to_span!(ast::UnaryOp);
expr_to_span!(ast::With);
