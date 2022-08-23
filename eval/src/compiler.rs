//! This module implements a compiler for compiling the rnix AST
//! representation to Tvix bytecode.
//!
//! A note on `unwrap()`: This module contains a lot of calls to
//! `unwrap()` or `expect(...)` on data structures returned by `rnix`.
//! The reason for this is that rnix uses the same data structures to
//! represent broken and correct ASTs, so all typed AST variants have
//! the ability to represent an incorrect node.
//!
//! However, at the time that the AST is passed to the compiler we
//! have verified that `rnix` considers the code to be correct, so all
//! variants are fulfilled. In cases where the invariant is guaranteed
//! by the code in this module, `debug_assert!` has been used to catch
//! mistakes early during development.

use path_clean::PathClean;
use rnix::ast::{self, AstToken, HasEntry};
use rowan::ast::AstNode;
use std::path::{Path, PathBuf};

use crate::chunk::Chunk;
use crate::errors::{Error, ErrorKind, EvalResult};
use crate::opcode::{CodeIdx, OpCode};
use crate::value::Value;
use crate::warnings::{EvalWarning, WarningKind};

/// Represents the result of compiling a piece of Nix code. If
/// compilation was successful, the resulting bytecode can be passed
/// to the VM.
pub struct CompilationResult {
    pub chunk: Chunk,
    pub warnings: Vec<EvalWarning>,
    pub errors: Vec<Error>,
}

// Represents a single local already known to the compiler.
struct Local {
    // Definition name, which can be different kinds of tokens (plain
    // string or identifier). Nix does not allow dynamic names inside
    // of `let`-expressions.
    name: String,

    // Syntax node at which this local was declared.
    node: Option<rnix::SyntaxNode>,

    // Scope depth of this local.
    depth: usize,

    // Phantom locals are not actually accessible by users (e.g.
    // intermediate values used for `with`).
    phantom: bool,

    // Is this local known to have been used at all?
    used: bool,
}

/// Represents a stack offset containing keys which are currently
/// in-scope through a with expression.
#[derive(Debug)]
struct With {
    depth: usize,
}

/// Represents a scope known during compilation, which can be resolved
/// directly to stack indices.
///
/// TODO(tazjin): `with`-stack
/// TODO(tazjin): flag "specials" (e.g. note depth if builtins are
/// overridden)
#[derive(Default)]
struct Scope {
    locals: Vec<Local>,

    // How many scopes "deep" are these locals?
    scope_depth: usize,

    // Stack indices of attribute sets currently in scope through
    // `with`.
    with_stack: Vec<With>,

    // Certain symbols are considered to be "poisoning" the scope when
    // defined. This is because users are allowed to override symbols
    // like 'true' or 'null'.
    //
    // To support this efficiently, the depth at which a poisoning
    // occured is tracked here.
    poisoned_true: usize,
    poisoned_false: usize,
    poisoned_null: usize,
}

struct Compiler {
    chunk: Chunk,
    scope: Scope,

    warnings: Vec<EvalWarning>,
    errors: Vec<Error>,
    root_dir: PathBuf,
}

impl Compiler {
    fn compile(&mut self, expr: ast::Expr) {
        match expr {
            ast::Expr::Literal(literal) => self.compile_literal(literal),
            ast::Expr::Path(path) => self.compile_path(path),
            ast::Expr::Str(s) => self.compile_str(s),
            ast::Expr::UnaryOp(op) => self.compile_unary_op(op),
            ast::Expr::BinOp(op) => self.compile_binop(op),
            ast::Expr::HasAttr(has_attr) => self.compile_has_attr(has_attr),
            ast::Expr::List(list) => self.compile_list(list),
            ast::Expr::AttrSet(attrs) => self.compile_attr_set(attrs),
            ast::Expr::Select(select) => self.compile_select(select),
            ast::Expr::Assert(assert) => self.compile_assert(assert),
            ast::Expr::IfElse(if_else) => self.compile_if_else(if_else),
            ast::Expr::LetIn(let_in) => self.compile_let_in(let_in),
            ast::Expr::Ident(ident) => self.compile_ident(ident),
            ast::Expr::With(with) => self.compile_with(with),

            // Parenthesized expressions are simply unwrapped, leaving
            // their value on the stack.
            ast::Expr::Paren(paren) => self.compile(paren.expr().unwrap()),

            ast::Expr::LegacyLet(_) => todo!("legacy let"),
            ast::Expr::Lambda(_) => todo!("function definition"),
            ast::Expr::Apply(_) => todo!("function application"),

            ast::Expr::Root(_) => unreachable!("there cannot be more than one root"),
            ast::Expr::Error(_) => unreachable!("compile is only called on validated trees"),
        }
    }

    fn compile_literal(&mut self, node: ast::Literal) {
        match node.kind() {
            ast::LiteralKind::Float(f) => {
                let idx = self.chunk.push_constant(Value::Float(f.value().unwrap()));
                self.chunk.push_op(OpCode::OpConstant(idx));
            }

            ast::LiteralKind::Integer(i) => {
                let idx = self.chunk.push_constant(Value::Integer(i.value().unwrap()));
                self.chunk.push_op(OpCode::OpConstant(idx));
            }
            ast::LiteralKind::Uri(u) => {
                self.emit_warning(node.syntax().clone(), WarningKind::DeprecatedLiteralURL);

                let idx = self
                    .chunk
                    .push_constant(Value::String(u.syntax().text().into()));
                self.chunk.push_op(OpCode::OpConstant(idx));
            }
        }
    }

    fn compile_path(&mut self, node: ast::Path) {
        // TODO(tazjin): placeholder implementation while waiting for
        // https://github.com/nix-community/rnix-parser/pull/96

        let raw_path = node.to_string();
        let path = if raw_path.starts_with('/') {
            Path::new(&raw_path).to_owned()
        } else if raw_path.starts_with('~') {
            let mut buf = match dirs::home_dir() {
                Some(buf) => buf,
                None => {
                    self.emit_error(
                        node.syntax().clone(),
                        ErrorKind::PathResolution("failed to determine home directory".into()),
                    );
                    return;
                }
            };

            buf.push(&raw_path);
            buf
        } else if raw_path.starts_with('.') {
            let mut buf = self.root_dir.clone();
            buf.push(&raw_path);
            buf
        } else {
            // TODO: decide what to do with findFile
            todo!("other path types (e.g. <...> lookups) not yet implemented")
        };

        // TODO: Use https://github.com/rust-lang/rfcs/issues/2208
        // once it is available
        let value = Value::Path(path.clean());
        let idx = self.chunk.push_constant(value);
        self.chunk.push_op(OpCode::OpConstant(idx));
    }

    fn compile_str(&mut self, node: ast::Str) {
        let mut count = 0;

        // The string parts are produced in literal order, however
        // they need to be reversed on the stack in order to
        // efficiently create the real string in case of
        // interpolation.
        for part in node.normalized_parts().into_iter().rev() {
            count += 1;

            match part {
                // Interpolated expressions are compiled as normal and
                // dealt with by the VM before being assembled into
                // the final string.
                ast::InterpolPart::Interpolation(node) => self.compile(node.expr().unwrap()),

                ast::InterpolPart::Literal(lit) => {
                    let idx = self.chunk.push_constant(Value::String(lit.into()));
                    self.chunk.push_op(OpCode::OpConstant(idx));
                }
            }
        }

        if count != 1 {
            self.chunk.push_op(OpCode::OpInterpolate(count));
        }
    }

    fn compile_unary_op(&mut self, op: ast::UnaryOp) {
        self.compile(op.expr().unwrap());

        let opcode = match op.operator().unwrap() {
            ast::UnaryOpKind::Invert => OpCode::OpInvert,
            ast::UnaryOpKind::Negate => OpCode::OpNegate,
        };

        self.chunk.push_op(opcode);
    }

    fn compile_binop(&mut self, op: ast::BinOp) {
        use ast::BinOpKind;

        // Short-circuiting and other strange operators, which are
        // under the same node type as NODE_BIN_OP, but need to be
        // handled separately (i.e. before compiling the expressions
        // used for standard binary operators).

        match op.operator().unwrap() {
            BinOpKind::And => return self.compile_and(op),
            BinOpKind::Or => return self.compile_or(op),
            BinOpKind::Implication => return self.compile_implication(op),
            _ => {}
        };

        // For all other operators, the two values need to be left on
        // the stack in the correct order before pushing the
        // instruction for the operation itself.
        self.compile(op.lhs().unwrap());
        self.compile(op.rhs().unwrap());

        match op.operator().unwrap() {
            BinOpKind::Add => self.chunk.push_op(OpCode::OpAdd),
            BinOpKind::Sub => self.chunk.push_op(OpCode::OpSub),
            BinOpKind::Mul => self.chunk.push_op(OpCode::OpMul),
            BinOpKind::Div => self.chunk.push_op(OpCode::OpDiv),
            BinOpKind::Update => self.chunk.push_op(OpCode::OpAttrsUpdate),
            BinOpKind::Equal => self.chunk.push_op(OpCode::OpEqual),
            BinOpKind::Less => self.chunk.push_op(OpCode::OpLess),
            BinOpKind::LessOrEq => self.chunk.push_op(OpCode::OpLessOrEq),
            BinOpKind::More => self.chunk.push_op(OpCode::OpMore),
            BinOpKind::MoreOrEq => self.chunk.push_op(OpCode::OpMoreOrEq),
            BinOpKind::Concat => self.chunk.push_op(OpCode::OpConcat),

            BinOpKind::NotEqual => {
                self.chunk.push_op(OpCode::OpEqual);
                self.chunk.push_op(OpCode::OpInvert)
            }

            // Handled by separate branch above.
            BinOpKind::And | BinOpKind::Implication | BinOpKind::Or => {
                unreachable!()
            }
        };
    }

    fn compile_and(&mut self, node: ast::BinOp) {
        debug_assert!(
            matches!(node.operator(), Some(ast::BinOpKind::And)),
            "compile_and called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack.
        self.compile(node.lhs().unwrap());

        // If this value is false, jump over the right-hand side - the
        // whole expression is false.
        let end_idx = self.chunk.push_op(OpCode::OpJumpIfFalse(0));

        // Otherwise, remove the previous value and leave the
        // right-hand side on the stack. Its result is now the value
        // of the whole expression.
        self.chunk.push_op(OpCode::OpPop);
        self.compile(node.rhs().unwrap());

        self.patch_jump(end_idx);
        self.chunk.push_op(OpCode::OpAssertBool);
    }

    fn compile_or(&mut self, node: ast::BinOp) {
        debug_assert!(
            matches!(node.operator(), Some(ast::BinOpKind::Or)),
            "compile_or called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack
        self.compile(node.lhs().unwrap());

        // Opposite of above: If this value is **true**, we can
        // short-circuit the right-hand side.
        let end_idx = self.chunk.push_op(OpCode::OpJumpIfTrue(0));
        self.chunk.push_op(OpCode::OpPop);
        self.compile(node.rhs().unwrap());
        self.patch_jump(end_idx);
        self.chunk.push_op(OpCode::OpAssertBool);
    }

    fn compile_implication(&mut self, node: ast::BinOp) {
        debug_assert!(
            matches!(node.operator(), Some(ast::BinOpKind::Implication)),
            "compile_implication called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack and invert it.
        self.compile(node.lhs().unwrap());
        self.chunk.push_op(OpCode::OpInvert);

        // Exactly as `||` (because `a -> b` = `!a || b`).
        let end_idx = self.chunk.push_op(OpCode::OpJumpIfTrue(0));
        self.chunk.push_op(OpCode::OpPop);
        self.compile(node.rhs().unwrap());
        self.patch_jump(end_idx);
        self.chunk.push_op(OpCode::OpAssertBool);
    }

    fn compile_has_attr(&mut self, node: ast::HasAttr) {
        // Put the attribute set on the stack.
        self.compile(node.expr().unwrap());
        let mut count = 0;

        // Push all path fragments with an operation for fetching the
        // next nested element, for all fragments except the last one.
        for fragment in node.attrpath().unwrap().attrs() {
            if count > 0 {
                self.chunk.push_op(OpCode::OpAttrOrNotFound);
            }
            count += 1;
            self.compile_attr(fragment);
        }

        // After the last fragment, emit the actual instruction that
        // leaves a boolean on the stack.
        self.chunk.push_op(OpCode::OpAttrsIsSet);
    }

    fn compile_attr(&mut self, node: ast::Attr) {
        match node {
            ast::Attr::Dynamic(dynamic) => self.compile(dynamic.expr().unwrap()),
            ast::Attr::Str(s) => self.compile_str(s),
            ast::Attr::Ident(ident) => self.emit_literal_ident(&ident),
        }
    }

    // Compile list literals into equivalent bytecode. List
    // construction is fairly simple, consisting of pushing code for
    // each literal element and an instruction with the element count.
    //
    // The VM, after evaluating the code for each element, simply
    // constructs the list from the given number of elements.
    fn compile_list(&mut self, node: ast::List) {
        let mut count = 0;

        for item in node.items() {
            count += 1;
            self.compile(item);
        }

        self.chunk.push_op(OpCode::OpList(count));
    }

    // Compile attribute set literals into equivalent bytecode.
    //
    // This is complicated by a number of features specific to Nix
    // attribute sets, most importantly:
    //
    // 1. Keys can be dynamically constructed through interpolation.
    // 2. Keys can refer to nested attribute sets.
    // 3. Attribute sets can (optionally) be recursive.
    fn compile_attr_set(&mut self, node: ast::AttrSet) {
        if node.rec_token().is_some() {
            todo!("recursive attribute sets are not yet implemented")
        }

        let mut count = 0;

        // Inherits have to be evaluated before entering the scope of
        // a potentially recursive attribute sets (i.e. we always
        // inherit "from the outside").
        for inherit in node.inherits() {
            match inherit.from() {
                Some(from) => {
                    for ident in inherit.idents() {
                        count += 1;

                        // First emit the identifier itself
                        self.emit_literal_ident(&ident);

                        // Then emit the node that we're inheriting
                        // from.
                        //
                        // TODO: Likely significant optimisation
                        // potential in having a multi-select
                        // instruction followed by a merge, rather
                        // than pushing/popping the same attrs
                        // potentially a lot of times.
                        self.compile(from.expr().unwrap());
                        self.emit_literal_ident(&ident);
                        self.chunk.push_op(OpCode::OpAttrsSelect);
                    }
                }

                None => {
                    for ident in inherit.idents() {
                        count += 1;
                        self.emit_literal_ident(&ident);

                        match self.resolve_local(ident.ident_token().unwrap().text()) {
                            Some(idx) => self.chunk.push_op(OpCode::OpGetLocal(idx)),
                            None => {
                                self.emit_error(
                                    ident.syntax().clone(),
                                    ErrorKind::UnknownStaticVariable,
                                );
                                continue;
                            }
                        };
                    }
                }
            }
        }

        for kv in node.attrpath_values() {
            count += 1;

            // Because attribute set literals can contain nested keys,
            // there is potentially more than one key fragment. If
            // this is the case, a special operation to construct a
            // runtime value representing the attribute path is
            // emitted.
            let mut key_count = 0;
            for fragment in kv.attrpath().unwrap().attrs() {
                key_count += 1;
                self.compile_attr(fragment);
            }

            // We're done with the key if there was only one fragment,
            // otherwise we need to emit an instruction to construct
            // the attribute path.
            if key_count > 1 {
                self.chunk.push_op(OpCode::OpAttrPath(key_count));
            }

            // The value is just compiled as normal so that its
            // resulting value is on the stack when the attribute set
            // is constructed at runtime.
            self.compile(kv.value().unwrap());
        }

        self.chunk.push_op(OpCode::OpAttrs(count));
    }

    fn compile_select(&mut self, node: ast::Select) {
        let set = node.expr().unwrap();
        let path = node.attrpath().unwrap();

        if node.or_token().is_some() {
            self.compile_select_or(set, path, node.default_expr().unwrap());
            return;
        }

        // Push the set onto the stack
        self.compile(set);

        // Compile each key fragment and emit access instructions.
        //
        // TODO: multi-select instruction to avoid re-pushing attrs on
        // nested selects.
        for fragment in path.attrs() {
            self.compile_attr(fragment);
            self.chunk.push_op(OpCode::OpAttrsSelect);
        }
    }

    /// Compile an `or` expression into a chunk of conditional jumps.
    ///
    /// If at any point during attribute set traversal a key is
    /// missing, the `OpAttrOrNotFound` instruction will leave a
    /// special sentinel value on the stack.
    ///
    /// After each access, a conditional jump evaluates the top of the
    /// stack and short-circuits to the default value if it sees the
    /// sentinel.
    ///
    /// Code like `{ a.b = 1; }.a.c or 42` yields this bytecode and
    /// runtime stack:
    ///
    /// ```notrust
    ///            Bytecode                     Runtime stack
    ///  ┌────────────────────────────┐   ┌─────────────────────────┐
    ///  │    ...                     │   │ ...                     │
    ///  │ 5  OP_ATTRS(1)             │ → │ 5  [ { a.b = 1; }     ] │
    ///  │ 6  OP_CONSTANT("a")        │ → │ 6  [ { a.b = 1; } "a" ] │
    ///  │ 7  OP_ATTR_OR_NOT_FOUND    │ → │ 7  [ { b = 1; }       ] │
    ///  │ 8  JUMP_IF_NOT_FOUND(13)   │ → │ 8  [ { b = 1; }       ] │
    ///  │ 9  OP_CONSTANT("C")        │ → │ 9  [ { b = 1; } "c"   ] │
    ///  │ 10 OP_ATTR_OR_NOT_FOUND    │ → │ 10 [ NOT_FOUND        ] │
    ///  │ 11 JUMP_IF_NOT_FOUND(13)   │ → │ 11 [                  ] │
    ///  │ 12 JUMP(14)                │   │ ..     jumped over      │
    ///  │ 13 CONSTANT(42)            │ → │ 12 [ 42 ]               │
    ///  │ 14 ...                     │   │ ..   ....               │
    ///  └────────────────────────────┘   └─────────────────────────┘
    /// ```
    fn compile_select_or(&mut self, set: ast::Expr, path: ast::Attrpath, default: ast::Expr) {
        self.compile(set);
        let mut jumps = vec![];

        for fragment in path.attrs() {
            self.compile_attr(fragment);
            self.chunk.push_op(OpCode::OpAttrOrNotFound);
            jumps.push(self.chunk.push_op(OpCode::OpJumpIfNotFound(0)));
        }

        let final_jump = self.chunk.push_op(OpCode::OpJump(0));

        for jump in jumps {
            self.patch_jump(jump);
        }

        // Compile the default value expression and patch the final
        // jump to point *beyond* it.
        self.compile(default);
        self.patch_jump(final_jump);
    }

    fn compile_assert(&mut self, node: ast::Assert) {
        // Compile the assertion condition to leave its value on the stack.
        self.compile(node.condition().unwrap());
        self.chunk.push_op(OpCode::OpAssert);

        // The runtime will abort evaluation at this point if the
        // assertion failed, if not the body simply continues on like
        // normal.
        self.compile(node.body().unwrap());
    }

    // Compile conditional expressions using jumping instructions in the VM.
    //
    //                        ┌────────────────────┐
    //                        │ 0  [ conditional ] │
    //                        │ 1   JUMP_IF_FALSE →┼─┐
    //                        │ 2  [  main body  ] │ │ Jump to else body if
    //                       ┌┼─3─←     JUMP       │ │ condition is false.
    //  Jump over else body  ││ 4  [  else body  ]←┼─┘
    //  if condition is true.└┼─5─→     ...        │
    //                        └────────────────────┘
    fn compile_if_else(&mut self, node: ast::IfElse) {
        self.compile(node.condition().unwrap());

        let then_idx = self.chunk.push_op(OpCode::OpJumpIfFalse(0));

        self.chunk.push_op(OpCode::OpPop); // discard condition value
        self.compile(node.body().unwrap());

        let else_idx = self.chunk.push_op(OpCode::OpJump(0));

        self.patch_jump(then_idx); // patch jump *to* else_body
        self.chunk.push_op(OpCode::OpPop); // discard condition value
        self.compile(node.else_body().unwrap());

        self.patch_jump(else_idx); // patch jump *over* else body
    }

    // Compile a standard `let ...; in ...` statement.
    //
    // Unless in a non-standard scope, the encountered values are
    // simply pushed on the stack and their indices noted in the
    // entries vector.
    fn compile_let_in(&mut self, node: ast::LetIn) {
        self.begin_scope();

        for inherit in node.inherits() {
            match inherit.from() {
                // Within a `let` binding, inheriting from the outer
                // scope is practically a no-op.
                None => {
                    self.emit_warning(inherit.syntax().clone(), WarningKind::UselessInherit);

                    continue;
                }

                Some(from) => {
                    for ident in inherit.idents() {
                        self.compile(from.expr().unwrap());
                        self.emit_literal_ident(&ident);
                        self.chunk.push_op(OpCode::OpAttrsSelect);
                        self.declare_local(
                            ident.syntax().clone(),
                            ident.ident_token().unwrap().text(),
                        );
                    }
                }
            }
        }

        for entry in node.attrpath_values() {
            let mut path = match normalise_ident_path(entry.attrpath().unwrap().attrs()) {
                Ok(p) => p,
                Err(err) => {
                    self.errors.push(err);
                    continue;
                }
            };

            if path.len() != 1 {
                todo!("nested bindings in let expressions :(")
            }

            self.compile(entry.value().unwrap());
            self.declare_local(
                entry.attrpath().unwrap().syntax().clone(),
                path.pop().unwrap(),
            );
        }

        // Deal with the body, then clean up the locals afterwards.
        self.compile(node.body().unwrap());
        self.end_scope();
    }

    fn compile_ident(&mut self, node: ast::Ident) {
        match node.ident_token().unwrap().text() {
            // TODO(tazjin): Nix technically allows code like
            //
            //   let null = 1; in null
            //   => 1
            //
            // which we do *not* want to check at runtime. Once
            // scoping is introduced, the compiler should carry some
            // optimised information about any "weird" stuff that's
            // happened to the scope (such as overrides of these
            // literals, or builtins).
            "true" if self.scope.poisoned_true == 0 => self.chunk.push_op(OpCode::OpTrue),
            "false" if self.scope.poisoned_false == 0 => self.chunk.push_op(OpCode::OpFalse),
            "null" if self.scope.poisoned_null == 0 => self.chunk.push_op(OpCode::OpNull),

            name => {
                // Note: `with` and some other special scoping
                // features are not yet implemented.
                match self.resolve_local(name) {
                    Some(idx) => self.chunk.push_op(OpCode::OpGetLocal(idx)),
                    None => {
                        if self.scope.with_stack.is_empty() {
                            self.emit_error(
                                node.syntax().clone(),
                                ErrorKind::UnknownStaticVariable,
                            );
                            return;
                        }

                        // Variable needs to be dynamically resolved
                        // at runtime.
                        let idx = self.chunk.push_constant(Value::String(name.into()));
                        self.chunk.push_op(OpCode::OpConstant(idx));
                        self.chunk.push_op(OpCode::OpResolveWith)
                    }
                }
            }
        };
    }

    // Compile `with` expressions by emitting instructions that
    // pop/remove the indices of attribute sets that are implicitly in
    // scope through `with` on the "with-stack".
    fn compile_with(&mut self, node: ast::With) {
        // TODO: Detect if the namespace is just an identifier, and
        // resolve that directly (thus avoiding duplication on the
        // stack).
        self.compile(node.namespace().unwrap());

        self.declare_phantom();
        self.scope.with_stack.push(With {
            depth: self.scope.scope_depth,
        });

        self.chunk
            .push_op(OpCode::OpPushWith(self.scope.locals.len() - 1));

        self.compile(node.body().unwrap());
    }

    /// Emit the literal string value of an identifier. Required for
    /// several operations related to attribute sets, where
    /// identifiers are used as string keys.
    fn emit_literal_ident(&mut self, ident: &ast::Ident) {
        let idx = self
            .chunk
            .push_constant(Value::String(ident.ident_token().unwrap().text().into()));

        self.chunk.push_op(OpCode::OpConstant(idx));
    }

    fn patch_jump(&mut self, idx: CodeIdx) {
        let offset = self.chunk.code.len() - 1 - idx.0;

        match &mut self.chunk.code[idx.0] {
            OpCode::OpJump(n)
            | OpCode::OpJumpIfFalse(n)
            | OpCode::OpJumpIfTrue(n)
            | OpCode::OpJumpIfNotFound(n) => {
                *n = offset;
            }

            op => panic!("attempted to patch unsupported op: {:?}", op),
        }
    }

    fn begin_scope(&mut self) {
        self.scope.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        debug_assert!(self.scope.scope_depth != 0, "can not end top scope");

        // If this scope poisoned any builtins or special identifiers,
        // they need to be reset.
        if self.scope.poisoned_true == self.scope.scope_depth {
            self.scope.poisoned_true = 0;
        }
        if self.scope.poisoned_false == self.scope.scope_depth {
            self.scope.poisoned_false = 0;
        }
        if self.scope.poisoned_null == self.scope.scope_depth {
            self.scope.poisoned_null = 0;
        }

        self.scope.scope_depth -= 1;

        // When ending a scope, all corresponding locals need to be
        // removed, but the value of the body needs to remain on the
        // stack. This is implemented by a separate instruction.
        let mut pops = 0;

        // TL;DR - iterate from the back while things belonging to the
        // ended scope still exist.
        while !self.scope.locals.is_empty()
            && self.scope.locals[self.scope.locals.len() - 1].depth > self.scope.scope_depth
        {
            pops += 1;

            // While removing the local, analyse whether it has been
            // accessed while it existed and emit a warning to the
            // user otherwise.
            if let Some(Local {
                node: Some(node),
                used,
                ..
            }) = self.scope.locals.pop()
            {
                if !used {
                    self.emit_warning(node, WarningKind::UnusedBinding);
                }
            }
        }

        if pops > 0 {
            self.chunk.push_op(OpCode::OpCloseScope(pops));
        }

        while !self.scope.with_stack.is_empty()
            && self.scope.with_stack[self.scope.with_stack.len() - 1].depth > self.scope.scope_depth
        {
            self.chunk.push_op(OpCode::OpPopWith);
            self.scope.with_stack.pop();
        }
    }

    /// Declare a local variable known in the scope that is being
    /// compiled by pushing it to the locals. This is used to
    /// determine the stack offset of variables.
    fn declare_local<S: Into<String>>(&mut self, node: rnix::SyntaxNode, name: S) {
        // Set up scope poisoning if required.
        let name = name.into();
        match name.as_str() {
            "true" if self.scope.poisoned_true == 0 => {
                self.scope.poisoned_true = self.scope.scope_depth
            }

            "false" if self.scope.poisoned_false == 0 => {
                self.scope.poisoned_false = self.scope.scope_depth
            }

            "null" if self.scope.poisoned_null == 0 => {
                self.scope.poisoned_null = self.scope.scope_depth
            }

            _ => {}
        };

        self.scope.locals.push(Local {
            name: name.into(),
            node: Some(node),
            depth: self.scope.scope_depth,
            phantom: false,
            used: false,
        });
    }

    fn declare_phantom(&mut self) {
        self.scope.locals.push(Local {
            name: "".into(),
            node: None,
            depth: self.scope.scope_depth,
            phantom: true,
            used: true,
        });
    }

    fn resolve_local(&mut self, name: &str) -> Option<usize> {
        let scope = &mut self.scope;

        for (idx, local) in scope.locals.iter_mut().enumerate().rev() {
            if !local.phantom && local.name == name {
                local.used = true;
                return Some(idx);
            }
        }

        None
    }

    fn emit_warning(&mut self, node: rnix::SyntaxNode, kind: WarningKind) {
        self.warnings.push(EvalWarning { node, kind })
    }

    fn emit_error(&mut self, node: rnix::SyntaxNode, kind: ErrorKind) {
        self.errors.push(Error {
            node: Some(node),
            kind,
        })
    }
}

/// Convert a non-dynamic string expression to a string if possible,
/// or raise an error.
fn expr_str_to_string(expr: ast::Str) -> EvalResult<String> {
    if expr.normalized_parts().len() == 1 {
        if let ast::InterpolPart::Literal(s) = expr.normalized_parts().pop().unwrap() {
            return Ok(s);
        }
    }

    return Err(Error {
        node: Some(expr.syntax().clone()),
        kind: ErrorKind::DynamicKeyInLet(expr.syntax().clone()),
    });
}

/// Convert a single identifier path fragment to a string if possible,
/// or raise an error about the node being dynamic.
fn attr_to_string(node: ast::Attr) -> EvalResult<String> {
    match node {
        ast::Attr::Ident(ident) => Ok(ident.ident_token().unwrap().text().into()),
        ast::Attr::Str(s) => expr_str_to_string(s),

        // The dynamic node type is just a wrapper. C++ Nix does not
        // care about the dynamic wrapper when determining whether the
        // node itself is dynamic, it depends solely on the expression
        // inside (i.e. `let ${"a"} = 1; in a` is valid).
        ast::Attr::Dynamic(ref dynamic) => match dynamic.expr().unwrap() {
            ast::Expr::Str(s) => expr_str_to_string(s),
            _ => Err(ErrorKind::DynamicKeyInLet(node.syntax().clone()).into()),
        },
    }
}

// Normalises identifier fragments into a single string vector for
// `let`-expressions; fails if fragments requiring dynamic computation
// are encountered.
fn normalise_ident_path<I: Iterator<Item = ast::Attr>>(path: I) -> EvalResult<Vec<String>> {
    path.map(attr_to_string).collect()
}

pub fn compile(expr: ast::Expr, location: Option<PathBuf>) -> EvalResult<CompilationResult> {
    let mut root_dir = match location {
        Some(dir) => Ok(dir),
        None => std::env::current_dir().map_err(|e| {
            ErrorKind::PathResolution(format!("could not determine current directory: {}", e))
        }),
    }?;

    // If the path passed from the caller points to a file, the
    // filename itself needs to be truncated as this must point to a
    // directory.
    if root_dir.is_file() {
        root_dir.pop();
    }

    let mut c = Compiler {
        root_dir,
        chunk: Chunk::default(),
        warnings: vec![],
        errors: vec![],
        scope: Default::default(),
    };

    c.compile(expr);

    Ok(CompilationResult {
        chunk: c.chunk,
        warnings: c.warnings,
        errors: c.errors,
    })
}
