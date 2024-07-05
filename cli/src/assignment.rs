use rnix::{Root, SyntaxKind, SyntaxNode};
use rowan::ast::AstNode;

/// An assignment of an identifier to a value in the context of a REPL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Assignment<'a> {
    pub(crate) ident: &'a str,
    pub(crate) value: rnix::ast::Expr,
}

impl<'a> Assignment<'a> {
    /// Try to parse an [`Assignment`] from the given input string.
    ///
    /// Returns [`None`] if the parsing fails for any reason, since the intent is for us to
    /// fall-back to trying to parse the input as a regular expression or other REPL commands for
    /// any reason, since the intent is for us to fall-back to trying to parse the input as a
    /// regular expression or other REPL command.
    pub fn parse(input: &'a str) -> Option<Self> {
        let mut tt = rnix::tokenizer::Tokenizer::new(input);
        macro_rules! next {
            ($kind:ident) => {{
                loop {
                    let (kind, tok) = tt.next()?;
                    if kind == SyntaxKind::TOKEN_WHITESPACE {
                        continue;
                    }
                    if kind != SyntaxKind::$kind {
                        return None;
                    }
                    break tok;
                }
            }};
        }

        let ident = next!(TOKEN_IDENT);
        let _equal = next!(TOKEN_ASSIGN);
        let (green, errs) = rnix::parser::parse(tt);
        let value = Root::cast(SyntaxNode::new_root(green))?.expr()?;

        if !errs.is_empty() {
            return None;
        }

        Some(Self { ident, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_assignments() {
        for input in ["x = 4", "x     =       \t\t\n\t4", "x=4"] {
            let res = Assignment::parse(input).unwrap();
            assert_eq!(res.ident, "x");
            assert_eq!(res.value.to_string(), "4");
        }
    }

    #[test]
    fn complex_exprs() {
        let input = "x = { y = 4; z = let q = 7; in [ q (y // { z = 9; }) ]; }";
        let res = Assignment::parse(input).unwrap();
        assert_eq!(res.ident, "x");
    }

    #[test]
    fn not_an_assignment() {
        let input = "{ x = 4; }";
        let res = Assignment::parse(input);
        assert!(res.is_none(), "{input:?}");
    }
}
