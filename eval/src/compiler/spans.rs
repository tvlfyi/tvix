//! Utilities for dealing with span tracking in the compiler.

use super::*;
use codemap::{File, Span};
use rowan::ast::AstNode;

/// Trait implemented by all types from which we can retrieve a span.
pub(super) trait ToSpan {
    fn span_for(&self, file: &File) -> Span;
}

impl ToSpan for Span {
    fn span_for(&self, _: &File) -> Span {
        *self
    }
}

impl ToSpan for rnix::SyntaxToken {
    fn span_for(&self, file: &File) -> Span {
        let rowan_span = self.text_range();
        file.span.subspan(
            u32::from(rowan_span.start()) as u64,
            u32::from(rowan_span.end()) as u64,
        )
    }
}

impl ToSpan for rnix::SyntaxNode {
    fn span_for(&self, file: &File) -> Span {
        let rowan_span = self.text_range();
        file.span.subspan(
            u32::from(rowan_span.start()) as u64,
            u32::from(rowan_span.end()) as u64,
        )
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

impl Compiler<'_> {
    pub(super) fn span_for<S: ToSpan>(&self, to_span: &S) -> Span {
        to_span.span_for(&self.file)
    }
}
