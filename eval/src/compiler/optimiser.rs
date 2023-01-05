//! Helper functions for extending the compiler with more linter-like
//! functionality while compiling (i.e. smarter warnings).

use super::*;

use ast::Expr;

/// Optimise the given expression where possible.
pub(super) fn optimise_expr(c: &mut Compiler, slot: LocalIdx, expr: ast::Expr) -> ast::Expr {
    match expr {
        Expr::BinOp(_) => optimise_bin_op(c, slot, expr),
        _ => expr.to_owned(),
    }
}

enum LitBool {
    Expr(Expr),
    True(Expr),
    False(Expr),
}

/// Is this a literal boolean, or something else?
fn is_lit_bool(expr: ast::Expr) -> LitBool {
    if let ast::Expr::Ident(ident) = &expr {
        match ident.ident_token().unwrap().text() {
            "true" => LitBool::True(expr),
            "false" => LitBool::False(expr),
            _ => LitBool::Expr(expr),
        }
    } else {
        LitBool::Expr(expr)
    }
}

/// Detect useless binary operations (i.e. useless bool comparisons).
fn optimise_bin_op(c: &mut Compiler, slot: LocalIdx, expr: ast::Expr) -> ast::Expr {
    use ast::BinOpKind;

    // bail out of this check if the user has poisoned either `true`
    // or `false` identifiers. Note that they will have received a
    // separate warning about this for shadowing the global(s).
    if c.scope().is_poisoned("true") || c.scope().is_poisoned("false") {
        return expr;
    }

    if let Expr::BinOp(op) = &expr {
        let lhs = is_lit_bool(op.lhs().unwrap());
        let rhs = is_lit_bool(op.rhs().unwrap());

        match (op.operator().unwrap(), lhs, rhs) {
            // useless `false` arm in `||` expression
            (BinOpKind::Or, LitBool::False(f), LitBool::Expr(other))
            | (BinOpKind::Or, LitBool::Expr(other), LitBool::False(f)) => {
                c.emit_warning(
                    &f,
                    WarningKind::UselessBoolOperation(
                        "this `false` has no effect on the result of the comparison",
                    ),
                );

                return other;
            }

            // useless `true` arm in `&&` expression
            (BinOpKind::And, LitBool::True(t), LitBool::Expr(other))
            | (BinOpKind::And, LitBool::Expr(other), LitBool::True(t)) => {
                c.emit_warning(
                    &t,
                    WarningKind::UselessBoolOperation(
                        "this `true` has no effect on the result of the comparison",
                    ),
                );

                return other;
            }

            // useless `||` expression (one arm is `true`), return
            // `true` directly (and warn about dead code on the right)
            (BinOpKind::Or, LitBool::True(t), LitBool::Expr(other)) => {
                c.emit_warning(
                    op,
                    WarningKind::UselessBoolOperation("this expression is always true"),
                );

                c.compile_dead_code(slot, other);

                return t;
            }

            (BinOpKind::Or, _, LitBool::True(t)) | (BinOpKind::Or, LitBool::True(t), _) => {
                c.emit_warning(
                    op,
                    WarningKind::UselessBoolOperation("this expression is always true"),
                );

                return t;
            }

            // useless `&&` expression (one arm is `false), same as above
            (BinOpKind::And, LitBool::False(f), LitBool::Expr(other)) => {
                c.emit_warning(
                    op,
                    WarningKind::UselessBoolOperation("this expression is always false"),
                );

                c.compile_dead_code(slot, other);

                return f;
            }

            (BinOpKind::And, _, LitBool::False(f)) | (BinOpKind::Or, LitBool::False(f), _) => {
                c.emit_warning(
                    op,
                    WarningKind::UselessBoolOperation("this expression is always false"),
                );

                return f;
            }

            _ => { /* nothing to optimise */ }
        }
    }

    expr
}
