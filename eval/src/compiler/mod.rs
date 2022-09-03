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

mod scope;

use path_clean::PathClean;
use rnix::ast::{self, AstToken, HasEntry};
use rowan::ast::AstNode;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::chunk::Chunk;
use crate::errors::{Error, ErrorKind, EvalResult};
use crate::opcode::{CodeIdx, Count, JumpOffset, OpCode, UpvalueIdx};
use crate::value::{Closure, Lambda, Thunk, Value};
use crate::warnings::{EvalWarning, WarningKind};

use self::scope::{LocalIdx, LocalPosition, Scope, Upvalue, UpvalueKind};

/// Represents the result of compiling a piece of Nix code. If
/// compilation was successful, the resulting bytecode can be passed
/// to the VM.
pub struct CompilationOutput {
    pub lambda: Lambda,
    pub warnings: Vec<EvalWarning>,
    pub errors: Vec<Error>,
}

/// Represents the lambda currently being compiled.
struct LambdaCtx {
    lambda: Lambda,
    scope: Scope,
}

impl LambdaCtx {
    fn new() -> Self {
        LambdaCtx {
            lambda: Lambda::new_anonymous(),
            scope: Default::default(),
        }
    }

    fn inherit(&self) -> Self {
        let ctx = LambdaCtx {
            lambda: Lambda::new_anonymous(),
            scope: self.scope.inherit(),
        };

        #[cfg(feature = "disassembler")]
        let ctx = (|mut c: Self| {
            c.lambda.chunk.codemap = self.lambda.chunk.codemap.clone();
            c
        })(ctx);

        ctx
    }
}

/// Alias for the map of globally available functions that should
/// implicitly be resolvable in the global scope.
type GlobalsMap = HashMap<&'static str, Rc<dyn Fn(&mut Compiler, rnix::ast::Ident)>>;

struct Compiler<'code> {
    contexts: Vec<LambdaCtx>,
    warnings: Vec<EvalWarning>,
    errors: Vec<Error>,
    root_dir: PathBuf,

    /// Carries all known global tokens; the full set of which is
    /// created when the compiler is invoked.
    ///
    /// Each global has an associated token, which when encountered as
    /// an identifier is resolved against the scope poisoning logic,
    /// and a function that should emit code for the token.
    globals: GlobalsMap,

    /// File reference in the codemap contains all known source code
    /// and is used to track the spans from which instructions where
    /// derived.
    file: &'code codemap::File,

    #[cfg(feature = "disassembler")]
    /// Carry a reference to the codemap around when the disassembler
    /// is enabled, to allow displaying lines and other source
    /// information in the disassembler output.
    codemap: Rc<codemap::CodeMap>,
}

// Helper functions for emitting code and metadata to the internal
// structures of the compiler.
impl Compiler<'_> {
    fn context(&self) -> &LambdaCtx {
        &self.contexts[self.contexts.len() - 1]
    }

    fn context_mut(&mut self) -> &mut LambdaCtx {
        let idx = self.contexts.len() - 1;
        &mut self.contexts[idx]
    }

    fn chunk(&mut self) -> &mut Chunk {
        &mut self.context_mut().lambda.chunk
    }

    fn scope(&self) -> &Scope {
        &self.context().scope
    }

    fn scope_mut(&mut self) -> &mut Scope {
        &mut self.context_mut().scope
    }

    fn span_for<N: AstNode>(&self, node: &N) -> codemap::Span {
        let rowan_span = node.syntax().text_range();
        self.file.span.subspan(
            u32::from(rowan_span.start()) as u64,
            u32::from(rowan_span.end()) as u64,
        )
    }

    /// Push a single instruction to the current bytecode chunk and
    /// track the source span from which it was compiled.
    fn push_op<T: AstNode>(&mut self, data: OpCode, node: &T) -> CodeIdx {
        let span = self.span_for(node);
        self.chunk().push_op(data, span)
    }

    /// Emit a single constant to the current bytecode chunk and track
    /// the source span from which it was compiled.
    fn emit_constant<T: AstNode>(&mut self, value: Value, node: &T) {
        let idx = self.chunk().push_constant(value);
        self.push_op(OpCode::OpConstant(idx), node);
    }
}

// Actual code-emitting AST traversal methods.
impl Compiler<'_> {
    fn compile(&mut self, slot: LocalIdx, expr: ast::Expr) {
        match expr {
            ast::Expr::Literal(literal) => self.compile_literal(literal),
            ast::Expr::Path(path) => self.compile_path(path),
            ast::Expr::Str(s) => self.compile_str(slot, s),
            ast::Expr::UnaryOp(op) => self.compile_unary_op(slot, op),
            ast::Expr::BinOp(op) => self.compile_binop(slot, op),
            ast::Expr::HasAttr(has_attr) => self.compile_has_attr(slot, has_attr),
            ast::Expr::List(list) => self.compile_list(slot, list),
            ast::Expr::AttrSet(attrs) => self.thunk(slot, &attrs, move |c, a, s| {
                c.compile_attr_set(s, a.clone())
            }),
            ast::Expr::Select(select) => self.compile_select(slot, select),
            ast::Expr::Assert(assert) => self.compile_assert(slot, assert),
            ast::Expr::IfElse(if_else) => self.compile_if_else(slot, if_else),
            ast::Expr::LetIn(let_in) => self.compile_let_in(slot, let_in),
            ast::Expr::Ident(ident) => self.compile_ident(slot, ident),
            ast::Expr::With(with) => self.compile_with(slot, with),
            ast::Expr::Lambda(lambda) => self.compile_lambda(slot, lambda),
            ast::Expr::Apply(apply) => self.compile_apply(slot, apply),

            // Parenthesized expressions are simply unwrapped, leaving
            // their value on the stack.
            ast::Expr::Paren(paren) => self.compile(slot, paren.expr().unwrap()),

            ast::Expr::LegacyLet(_) => todo!("legacy let"),

            ast::Expr::Root(_) => unreachable!("there cannot be more than one root"),
            ast::Expr::Error(_) => unreachable!("compile is only called on validated trees"),
        }
    }

    fn compile_literal(&mut self, node: ast::Literal) {
        match node.kind() {
            ast::LiteralKind::Float(f) => {
                self.emit_constant(Value::Float(f.value().unwrap()), &node);
            }

            ast::LiteralKind::Integer(i) => {
                self.emit_constant(Value::Integer(i.value().unwrap()), &node);
            }

            ast::LiteralKind::Uri(u) => {
                self.emit_warning(self.span_for(&node), WarningKind::DeprecatedLiteralURL);
                self.emit_constant(Value::String(u.syntax().text().into()), &node);
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
                        self.span_for(&node),
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
        self.emit_constant(value, &node);
    }

    fn compile_str(&mut self, slot: LocalIdx, node: ast::Str) {
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
                ast::InterpolPart::Interpolation(node) => self.compile(slot, node.expr().unwrap()),

                ast::InterpolPart::Literal(lit) => {
                    self.emit_constant(Value::String(lit.into()), &node);
                }
            }
        }

        if count != 1 {
            self.push_op(OpCode::OpInterpolate(Count(count)), &node);
        }
    }

    fn compile_unary_op(&mut self, slot: LocalIdx, op: ast::UnaryOp) {
        self.compile(slot, op.expr().unwrap());
        self.emit_force(&op);

        let opcode = match op.operator().unwrap() {
            ast::UnaryOpKind::Invert => OpCode::OpInvert,
            ast::UnaryOpKind::Negate => OpCode::OpNegate,
        };

        self.push_op(opcode, &op);
    }

    fn compile_binop(&mut self, slot: LocalIdx, op: ast::BinOp) {
        use ast::BinOpKind;

        // Short-circuiting and other strange operators, which are
        // under the same node type as NODE_BIN_OP, but need to be
        // handled separately (i.e. before compiling the expressions
        // used for standard binary operators).

        match op.operator().unwrap() {
            BinOpKind::And => return self.compile_and(slot, op),
            BinOpKind::Or => return self.compile_or(slot, op),
            BinOpKind::Implication => return self.compile_implication(slot, op),
            _ => {}
        };

        // For all other operators, the two values need to be left on
        // the stack in the correct order before pushing the
        // instruction for the operation itself.
        self.compile(slot, op.lhs().unwrap());
        self.emit_force(&op.lhs().unwrap());

        self.compile(slot, op.rhs().unwrap());
        self.emit_force(&op.rhs().unwrap());

        match op.operator().unwrap() {
            BinOpKind::Add => self.push_op(OpCode::OpAdd, &op),
            BinOpKind::Sub => self.push_op(OpCode::OpSub, &op),
            BinOpKind::Mul => self.push_op(OpCode::OpMul, &op),
            BinOpKind::Div => self.push_op(OpCode::OpDiv, &op),
            BinOpKind::Update => self.push_op(OpCode::OpAttrsUpdate, &op),
            BinOpKind::Equal => self.push_op(OpCode::OpEqual, &op),
            BinOpKind::Less => self.push_op(OpCode::OpLess, &op),
            BinOpKind::LessOrEq => self.push_op(OpCode::OpLessOrEq, &op),
            BinOpKind::More => self.push_op(OpCode::OpMore, &op),
            BinOpKind::MoreOrEq => self.push_op(OpCode::OpMoreOrEq, &op),
            BinOpKind::Concat => self.push_op(OpCode::OpConcat, &op),

            BinOpKind::NotEqual => {
                self.push_op(OpCode::OpEqual, &op);
                self.push_op(OpCode::OpInvert, &op)
            }

            // Handled by separate branch above.
            BinOpKind::And | BinOpKind::Implication | BinOpKind::Or => {
                unreachable!()
            }
        };
    }

    fn compile_and(&mut self, slot: LocalIdx, node: ast::BinOp) {
        debug_assert!(
            matches!(node.operator(), Some(ast::BinOpKind::And)),
            "compile_and called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack.
        self.compile(slot, node.lhs().unwrap());
        self.emit_force(&node.lhs().unwrap());

        // If this value is false, jump over the right-hand side - the
        // whole expression is false.
        let end_idx = self.push_op(OpCode::OpJumpIfFalse(JumpOffset(0)), &node);

        // Otherwise, remove the previous value and leave the
        // right-hand side on the stack. Its result is now the value
        // of the whole expression.
        self.push_op(OpCode::OpPop, &node);
        self.compile(slot, node.rhs().unwrap());
        self.emit_force(&node.rhs().unwrap());

        self.patch_jump(end_idx);
        self.push_op(OpCode::OpAssertBool, &node);
    }

    fn compile_or(&mut self, slot: LocalIdx, node: ast::BinOp) {
        debug_assert!(
            matches!(node.operator(), Some(ast::BinOpKind::Or)),
            "compile_or called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack
        self.compile(slot, node.lhs().unwrap());
        self.emit_force(&node.lhs().unwrap());

        // Opposite of above: If this value is **true**, we can
        // short-circuit the right-hand side.
        let end_idx = self.push_op(OpCode::OpJumpIfTrue(JumpOffset(0)), &node);
        self.push_op(OpCode::OpPop, &node);
        self.compile(slot, node.rhs().unwrap());
        self.emit_force(&node.rhs().unwrap());

        self.patch_jump(end_idx);
        self.push_op(OpCode::OpAssertBool, &node);
    }

    fn compile_implication(&mut self, slot: LocalIdx, node: ast::BinOp) {
        debug_assert!(
            matches!(node.operator(), Some(ast::BinOpKind::Implication)),
            "compile_implication called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack and invert it.
        self.compile(slot, node.lhs().unwrap());
        self.emit_force(&node.lhs().unwrap());
        self.push_op(OpCode::OpInvert, &node);

        // Exactly as `||` (because `a -> b` = `!a || b`).
        let end_idx = self.push_op(OpCode::OpJumpIfTrue(JumpOffset(0)), &node);
        self.push_op(OpCode::OpPop, &node);
        self.compile(slot, node.rhs().unwrap());
        self.emit_force(&node.rhs().unwrap());

        self.patch_jump(end_idx);
        self.push_op(OpCode::OpAssertBool, &node);
    }

    fn compile_has_attr(&mut self, slot: LocalIdx, node: ast::HasAttr) {
        // Put the attribute set on the stack.
        self.compile(slot, node.expr().unwrap());

        // Push all path fragments with an operation for fetching the
        // next nested element, for all fragments except the last one.
        for (count, fragment) in node.attrpath().unwrap().attrs().enumerate() {
            if count > 0 {
                self.push_op(OpCode::OpAttrsTrySelect, &fragment);
            }

            self.compile_attr(slot, fragment);
        }

        // After the last fragment, emit the actual instruction that
        // leaves a boolean on the stack.
        self.push_op(OpCode::OpAttrsIsSet, &node);
    }

    fn compile_attr(&mut self, slot: LocalIdx, node: ast::Attr) {
        match node {
            ast::Attr::Dynamic(dynamic) => {
                self.compile(slot, dynamic.expr().unwrap());
                self.emit_force(&dynamic.expr().unwrap());
            }

            ast::Attr::Str(s) => {
                self.compile_str(slot, s.clone());
                self.emit_force(&s);
            }

            ast::Attr::Ident(ident) => self.emit_literal_ident(&ident),
        }
    }

    // Compile list literals into equivalent bytecode. List
    // construction is fairly simple, consisting of pushing code for
    // each literal element and an instruction with the element count.
    //
    // The VM, after evaluating the code for each element, simply
    // constructs the list from the given number of elements.
    fn compile_list(&mut self, slot: LocalIdx, node: ast::List) {
        let mut count = 0;

        for item in node.items() {
            count += 1;
            self.compile(slot, item);
        }

        self.push_op(OpCode::OpList(Count(count)), &node);
    }

    // Compile attribute set literals into equivalent bytecode.
    //
    // This is complicated by a number of features specific to Nix
    // attribute sets, most importantly:
    //
    // 1. Keys can be dynamically constructed through interpolation.
    // 2. Keys can refer to nested attribute sets.
    // 3. Attribute sets can (optionally) be recursive.
    fn compile_attr_set(&mut self, slot: LocalIdx, node: ast::AttrSet) {
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

                        // First emit the identifier itself (this
                        // becomes the new key).
                        self.emit_literal_ident(&ident);

                        // Then emit the node that we're inheriting
                        // from.
                        //
                        // TODO: Likely significant optimisation
                        // potential in having a multi-select
                        // instruction followed by a merge, rather
                        // than pushing/popping the same attrs
                        // potentially a lot of times.
                        self.compile(slot, from.expr().unwrap());
                        self.emit_force(&from.expr().unwrap());
                        self.emit_literal_ident(&ident);
                        self.push_op(OpCode::OpAttrsSelect, &ident);
                    }
                }

                None => {
                    for ident in inherit.idents() {
                        count += 1;

                        // Emit the key to use for OpAttrs
                        self.emit_literal_ident(&ident);

                        // Emit the value.
                        self.compile_ident(slot, ident);
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
                self.compile_attr(slot, fragment);
            }

            // We're done with the key if there was only one fragment,
            // otherwise we need to emit an instruction to construct
            // the attribute path.
            if key_count > 1 {
                self.push_op(
                    OpCode::OpAttrPath(Count(key_count)),
                    &kv.attrpath().unwrap(),
                );
            }

            // The value is just compiled as normal so that its
            // resulting value is on the stack when the attribute set
            // is constructed at runtime.
            self.compile(slot, kv.value().unwrap());
        }

        self.push_op(OpCode::OpAttrs(Count(count)), &node);
    }

    fn compile_select(&mut self, slot: LocalIdx, node: ast::Select) {
        let set = node.expr().unwrap();
        let path = node.attrpath().unwrap();

        if node.or_token().is_some() {
            self.compile_select_or(slot, set, path, node.default_expr().unwrap());
            return;
        }

        // Push the set onto the stack
        self.compile(slot, set.clone());
        self.emit_force(&set);

        // Compile each key fragment and emit access instructions.
        //
        // TODO: multi-select instruction to avoid re-pushing attrs on
        // nested selects.
        for fragment in path.attrs() {
            self.compile_attr(slot, fragment);
            self.push_op(OpCode::OpAttrsSelect, &node);
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
    fn compile_select_or(
        &mut self,
        slot: LocalIdx,
        set: ast::Expr,
        path: ast::Attrpath,
        default: ast::Expr,
    ) {
        self.compile(slot, set.clone());
        self.emit_force(&set);
        let mut jumps = vec![];

        for fragment in path.attrs() {
            self.compile_attr(slot, fragment.clone());
            self.push_op(OpCode::OpAttrsTrySelect, &fragment);
            jumps.push(self.push_op(OpCode::OpJumpIfNotFound(JumpOffset(0)), &fragment));
        }

        let final_jump = self.push_op(OpCode::OpJump(JumpOffset(0)), &path);

        for jump in jumps {
            self.patch_jump(jump);
        }

        // Compile the default value expression and patch the final
        // jump to point *beyond* it.
        self.compile(slot, default);
        self.patch_jump(final_jump);
    }

    fn compile_assert(&mut self, slot: LocalIdx, node: ast::Assert) {
        // Compile the assertion condition to leave its value on the stack.
        self.compile(slot, node.condition().unwrap());
        self.push_op(OpCode::OpAssert, &node);

        // The runtime will abort evaluation at this point if the
        // assertion failed, if not the body simply continues on like
        // normal.
        self.compile(slot, node.body().unwrap());
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
    fn compile_if_else(&mut self, slot: LocalIdx, node: ast::IfElse) {
        self.compile(slot, node.condition().unwrap());

        let then_idx = self.push_op(
            OpCode::OpJumpIfFalse(JumpOffset(0)),
            &node.condition().unwrap(),
        );

        self.push_op(OpCode::OpPop, &node); // discard condition value
        self.compile(slot, node.body().unwrap());

        let else_idx = self.push_op(OpCode::OpJump(JumpOffset(0)), &node);

        self.patch_jump(then_idx); // patch jump *to* else_body
        self.push_op(OpCode::OpPop, &node); // discard condition value
        self.compile(slot, node.else_body().unwrap());

        self.patch_jump(else_idx); // patch jump *over* else body
    }

    // Compile an `inherit` node of a `let`-expression.
    fn compile_let_inherit<I: Iterator<Item = ast::Inherit>>(
        &mut self,
        slot: LocalIdx,
        inherits: I,
    ) {
        for inherit in inherits {
            match inherit.from() {
                // Within a `let` binding, inheriting from the outer
                // scope is a no-op *if* the identifier can be
                // statically resolved.
                None if !self.scope().has_with() => {
                    self.emit_warning(self.span_for(&inherit), WarningKind::UselessInherit);
                    continue;
                }

                None => {
                    for ident in inherit.idents() {
                        // If the identifier resolves statically, it
                        // has precedence over dynamic bindings, and
                        // the inherit is useless.
                        if matches!(
                            self.scope_mut()
                                .resolve_local(ident.ident_token().unwrap().text()),
                            LocalPosition::Known(_)
                        ) {
                            self.emit_warning(self.span_for(&ident), WarningKind::UselessInherit);
                            continue;
                        }

                        self.compile_ident(slot, ident.clone());
                        let idx = self.declare_local(&ident, ident.ident_token().unwrap().text());
                        self.scope_mut().mark_initialised(idx);
                    }
                }

                Some(from) => {
                    for ident in inherit.idents() {
                        self.compile(slot, from.expr().unwrap());
                        self.emit_force(&from.expr().unwrap());

                        self.emit_literal_ident(&ident);
                        self.push_op(OpCode::OpAttrsSelect, &ident);
                        let idx = self.declare_local(&ident, ident.ident_token().unwrap().text());
                        self.scope_mut().mark_initialised(idx);
                    }
                }
            }
        }
    }

    // Compile a standard `let ...; in ...` statement.
    //
    // Unless in a non-standard scope, the encountered values are
    // simply pushed on the stack and their indices noted in the
    // entries vector.
    fn compile_let_in(&mut self, slot: LocalIdx, node: ast::LetIn) {
        self.begin_scope();

        self.compile_let_inherit(slot, node.inherits());

        // First pass to ensure that all identifiers are known;
        // required for resolving recursion.
        let mut entries: Vec<(LocalIdx, ast::Expr)> = vec![];
        for entry in node.attrpath_values() {
            let mut path = match self.normalise_ident_path(entry.attrpath().unwrap().attrs()) {
                Ok(p) => p,
                Err(err) => {
                    self.errors.push(err);
                    continue;
                }
            };

            if path.len() != 1 {
                todo!("nested bindings in let expressions :(")
            }

            let idx = self.declare_local(&entry.attrpath().unwrap(), path.pop().unwrap());

            entries.push((idx, entry.value().unwrap()));
        }

        // Second pass to place the values in the correct stack slots.
        let indices: Vec<LocalIdx> = entries.iter().map(|(idx, _)| *idx).collect();
        for (idx, value) in entries.into_iter() {
            self.compile(idx, value);

            // Any code after this point will observe the value in the
            // right stack slot, so mark it as initialised.
            self.scope_mut().mark_initialised(idx);
        }

        // Third pass to emit finaliser instructions if necessary.
        for idx in indices {
            if self.scope()[idx].needs_finaliser {
                let stack_idx = self.scope().stack_index(idx);
                self.push_op(OpCode::OpFinalise(stack_idx), &node);
            }
        }

        // Deal with the body, then clean up the locals afterwards.
        self.compile(slot, node.body().unwrap());
        self.end_scope(&node);
    }

    fn compile_ident(&mut self, slot: LocalIdx, node: ast::Ident) {
        let ident = node.ident_token().unwrap();

        // If the identifier is a global, and it is not poisoned, emit
        // the global directly.
        if let Some(global) = self.globals.get(ident.text()) {
            if !self.scope().is_poisoned(ident.text()) {
                global.clone()(self, node.clone());
                return;
            }
        }

        match self.scope_mut().resolve_local(ident.text()) {
            LocalPosition::Unknown => {
                // Are we possibly dealing with an upvalue?
                if let Some(idx) =
                    self.resolve_upvalue(self.contexts.len() - 1, ident.text(), &node)
                {
                    self.push_op(OpCode::OpGetUpvalue(idx), &node);
                    return;
                }

                // Even worse - are we dealing with a dynamic upvalue?
                if let Some(idx) =
                    self.resolve_dynamic_upvalue(self.contexts.len() - 1, ident.text(), &node)
                {
                    // Edge case: Current scope *also* has a non-empty
                    // `with`-stack. This means we need to resolve
                    // both in this scope, and in the upvalues.
                    if self.scope().has_with() {
                        self.emit_literal_ident(&node);
                        self.push_op(OpCode::OpResolveWithOrUpvalue(idx), &node);
                        return;
                    }

                    self.push_op(OpCode::OpGetUpvalue(idx), &node);
                    return;
                }

                if !self.scope().has_with() {
                    self.emit_error(self.span_for(&node), ErrorKind::UnknownStaticVariable);
                    return;
                }

                // Variable needs to be dynamically resolved at
                // runtime.
                self.emit_literal_ident(&node);
                self.push_op(OpCode::OpResolveWith, &node);
            }

            LocalPosition::Known(idx) => {
                let stack_idx = self.scope().stack_index(idx);
                self.push_op(OpCode::OpGetLocal(stack_idx), &node);
            }

            // This identifier is referring to a value from the same
            // scope which is not yet defined. This identifier access
            // must be thunked.
            LocalPosition::Recursive(idx) => self.thunk(slot, &node, move |compiler, node, _| {
                let upvalue_idx = compiler.add_upvalue(
                    compiler.contexts.len() - 1,
                    &node,
                    UpvalueKind::Local(idx),
                );
                compiler.push_op(OpCode::OpGetUpvalue(upvalue_idx), node);
            }),
        };
    }

    // Compile `with` expressions by emitting instructions that
    // pop/remove the indices of attribute sets that are implicitly in
    // scope through `with` on the "with-stack".
    fn compile_with(&mut self, slot: LocalIdx, node: ast::With) {
        self.begin_scope();
        // TODO: Detect if the namespace is just an identifier, and
        // resolve that directly (thus avoiding duplication on the
        // stack).
        self.compile(slot, node.namespace().unwrap());
        self.emit_force(&node.namespace().unwrap());

        let span = self.span_for(&node.namespace().unwrap());

        // The attribute set from which `with` inherits values
        // occupies a slot on the stack, but this stack slot is not
        // directly accessible. As it must be accounted for to
        // calculate correct offsets, what we call a "phantom" local
        // is declared here.
        let local_idx = self.scope_mut().declare_phantom(span);
        self.scope_mut().mark_initialised(local_idx);
        let with_idx = self.scope().stack_index(local_idx);

        self.scope_mut().push_with();

        self.push_op(OpCode::OpPushWith(with_idx), &node);

        self.compile(slot, node.body().unwrap());

        self.push_op(OpCode::OpPopWith, &node);
        self.scope_mut().pop_with();
        self.end_scope(&node);
    }

    fn compile_lambda(&mut self, slot: LocalIdx, node: ast::Lambda) {
        self.new_context();
        self.begin_scope();

        // Compile the function itself
        match node.param().unwrap() {
            ast::Param::Pattern(_) => todo!("formals function definitions"),
            ast::Param::IdentParam(param) => {
                let name = param
                    .ident()
                    .unwrap()
                    .ident_token()
                    .unwrap()
                    .text()
                    .to_string();

                let idx = self.declare_local(&param, &name);
                self.scope_mut().mark_initialised(idx);
            }
        }

        self.compile(slot, node.body().unwrap());
        self.end_scope(&node);

        // TODO: determine and insert enclosing name, if available.

        // Pop the lambda context back off, and emit the finished
        // lambda as a constant.
        let compiled = self.contexts.pop().unwrap();

        #[cfg(feature = "disassembler")]
        {
            crate::disassembler::disassemble_chunk(&compiled.lambda.chunk);
        }

        // If the function is not a closure, just emit it directly and
        // move on.
        if compiled.lambda.upvalue_count == 0 {
            self.emit_constant(
                Value::Closure(Closure::new(Rc::new(compiled.lambda))),
                &node,
            );
            return;
        }

        // If the function is a closure, we need to emit the variable
        // number of operands that allow the runtime to close over the
        // upvalues and leave a blueprint in the constant index from
        // which the runtime closure can be constructed.
        let blueprint_idx = self
            .chunk()
            .push_constant(Value::Blueprint(Rc::new(compiled.lambda)));

        self.push_op(OpCode::OpClosure(blueprint_idx), &node);
        self.emit_upvalue_data(slot, compiled.scope.upvalues);
    }

    fn compile_apply(&mut self, slot: LocalIdx, node: ast::Apply) {
        // To call a function, we leave its arguments on the stack,
        // followed by the function expression itself, and then emit a
        // call instruction. This way, the stack is perfectly laid out
        // to enter the function call straight away.
        self.compile(slot, node.argument().unwrap());
        self.compile(slot, node.lambda().unwrap());
        self.push_op(OpCode::OpCall, &node);
    }

    /// Compile an expression into a runtime thunk which should be
    /// lazily evaluated when accessed.
    // TODO: almost the same as Compiler::compile_lambda; unify?
    fn thunk<N, F>(&mut self, slot: LocalIdx, node: &N, content: F)
    where
        N: AstNode + Clone,
        F: FnOnce(&mut Compiler, &N, LocalIdx),
    {
        self.new_context();
        self.begin_scope();
        content(self, node, slot);
        self.end_scope(node);

        let thunk = self.contexts.pop().unwrap();

        #[cfg(feature = "disassembler")]
        {
            crate::disassembler::disassemble_chunk(&thunk.lambda.chunk);
        }

        // Emit the thunk directly if it does not close over the
        // environment.
        if thunk.lambda.upvalue_count == 0 {
            self.emit_constant(Value::Thunk(Thunk::new(Rc::new(thunk.lambda))), node);
            return;
        }

        // Otherwise prepare for runtime construction of the thunk.
        let blueprint_idx = self
            .chunk()
            .push_constant(Value::Blueprint(Rc::new(thunk.lambda)));

        self.push_op(OpCode::OpThunk(blueprint_idx), node);
        self.emit_upvalue_data(slot, thunk.scope.upvalues);
    }

    /// Emit the data instructions that the runtime needs to correctly
    /// assemble the provided upvalues array.
    fn emit_upvalue_data(&mut self, slot: LocalIdx, upvalues: Vec<Upvalue>) {
        let this_stack_slot = self.scope().stack_index(slot);
        for upvalue in upvalues {
            match upvalue.kind {
                UpvalueKind::Local(idx) => {
                    let stack_idx = self.scope().stack_index(idx);

                    // If the upvalue slot is located *after* the
                    // closure, the upvalue resolution must be
                    // deferred until the scope is fully initialised
                    // and can be finalised.
                    if this_stack_slot < stack_idx {
                        self.push_op(OpCode::DataDeferredLocal(stack_idx), &upvalue.node);
                        self.scope_mut().mark_needs_finaliser(slot);
                    } else {
                        self.push_op(OpCode::DataLocalIdx(stack_idx), &upvalue.node);
                    }
                }

                UpvalueKind::Upvalue(idx) => {
                    self.push_op(OpCode::DataUpvalueIdx(idx), &upvalue.node);
                }

                UpvalueKind::Dynamic { name, up } => {
                    let idx = self.chunk().push_constant(Value::String(name.into()));
                    self.push_op(OpCode::DataDynamicIdx(idx), &upvalue.node);
                    if let Some(up) = up {
                        self.push_op(OpCode::DataDynamicAncestor(up), &upvalue.node);
                    }
                }
            };
        }
    }

    /// Emit the literal string value of an identifier. Required for
    /// several operations related to attribute sets, where
    /// identifiers are used as string keys.
    fn emit_literal_ident(&mut self, ident: &ast::Ident) {
        self.emit_constant(
            Value::String(ident.ident_token().unwrap().text().into()),
            ident,
        );
    }

    /// Patch the jump instruction at the given index, setting its
    /// jump offset from the placeholder to the current code position.
    ///
    /// This is required because the actual target offset of jumps is
    /// not known at the time when the jump operation itself is
    /// emitted.
    fn patch_jump(&mut self, idx: CodeIdx) {
        let offset = JumpOffset(self.chunk().code.len() - 1 - idx.0);

        match &mut self.chunk().code[idx.0] {
            OpCode::OpJump(n)
            | OpCode::OpJumpIfFalse(n)
            | OpCode::OpJumpIfTrue(n)
            | OpCode::OpJumpIfNotFound(n) => {
                *n = offset;
            }

            op => panic!("attempted to patch unsupported op: {:?}", op),
        }
    }

    /// Increase the scope depth of the current function (e.g. within
    /// a new bindings block, or `with`-scope).
    fn begin_scope(&mut self) {
        self.scope_mut().scope_depth += 1;
    }

    /// Decrease scope depth of the current function and emit
    /// instructions to clean up the stack at runtime.
    fn end_scope<N: AstNode>(&mut self, node: &N) {
        debug_assert!(self.scope().scope_depth != 0, "can not end top scope");

        // If this scope poisoned any builtins or special identifiers,
        // they need to be reset.
        let depth = self.scope().scope_depth;
        self.scope_mut().unpoison(depth);

        self.scope_mut().scope_depth -= 1;

        // When ending a scope, all corresponding locals need to be
        // removed, but the value of the body needs to remain on the
        // stack. This is implemented by a separate instruction.
        let mut pops = 0;

        // TL;DR - iterate from the back while things belonging to the
        // ended scope still exist.
        while !self.scope().locals.is_empty()
            && self.scope().locals[self.scope().locals.len() - 1].above(self.scope().scope_depth)
        {
            pops += 1;

            // While removing the local, analyse whether it has been
            // accessed while it existed and emit a warning to the
            // user otherwise.
            if let Some(local) = self.scope_mut().locals.pop() {
                if !local.used && !local.is_ignored() {
                    self.emit_warning(local.span, WarningKind::UnusedBinding);
                }
            }
        }

        if pops > 0 {
            self.push_op(OpCode::OpCloseScope(Count(pops)), node);
        }
    }

    /// Open a new lambda context within which to compile a function,
    /// closure or thunk.
    fn new_context(&mut self) {
        // This must inherit the scope-poisoning status of the parent
        // in order for upvalue resolution to work correctly with
        // poisoned identifiers.
        self.contexts.push(self.context().inherit());
    }

    /// Declare a local variable known in the scope that is being
    /// compiled by pushing it to the locals. This is used to
    /// determine the stack offset of variables.
    fn declare_local<S: Into<String>, N: AstNode>(&mut self, node: &N, name: S) -> LocalIdx {
        let name = name.into();
        let depth = self.scope().scope_depth;

        // Do this little dance to get ahold of the *static* key and
        // use it for poisoning if required.
        let key: Option<&'static str> = match self.globals.get_key_value(name.as_str()) {
            Some((key, _)) => Some(*key),
            None => None,
        };

        if let Some(global_ident) = key {
            self.emit_warning(
                self.span_for(node),
                WarningKind::ShadowedGlobal(global_ident),
            );
            self.scope_mut().poison(global_ident, depth);
        }

        let mut shadowed = false;
        for other in self.scope().locals.iter().rev() {
            if other.has_name(&name) && other.depth == depth {
                shadowed = true;
                break;
            }
        }

        if shadowed {
            self.emit_error(
                self.span_for(node),
                ErrorKind::VariableAlreadyDefined(name.clone()),
            );
        }

        let span = self.span_for(node);
        self.scope_mut().declare_local(name, span)
    }

    fn resolve_upvalue(
        &mut self,
        ctx_idx: usize,
        name: &str,
        node: &rnix::ast::Ident,
    ) -> Option<UpvalueIdx> {
        if ctx_idx == 0 {
            // There can not be any upvalue at the outermost context.
            return None;
        }

        // Determine whether the upvalue is a local in the enclosing context.
        match self.contexts[ctx_idx - 1].scope.resolve_local(name) {
            // recursive upvalues are dealt with the same way as
            // standard known ones, as thunks and closures are
            // guaranteed to be placed on the stack (i.e. in the right
            // position) *during* their runtime construction
            LocalPosition::Known(idx) | LocalPosition::Recursive(idx) => {
                return Some(self.add_upvalue(ctx_idx, node, UpvalueKind::Local(idx)))
            }

            LocalPosition::Unknown => { /* continue below */ }
        };

        // If the upvalue comes from even further up, we need to
        // recurse to make sure that the upvalues are created at each
        // level.
        if let Some(idx) = self.resolve_upvalue(ctx_idx - 1, name, node) {
            return Some(self.add_upvalue(ctx_idx, node, UpvalueKind::Upvalue(idx)));
        }

        None
    }

    /// If no static resolution for a potential upvalue was found,
    /// finds the lowest lambda context that has a `with`-stack and
    /// thread dynamic upvalues all the way through.
    ///
    /// At runtime, as closures are being constructed they either
    /// capture a dynamically available upvalue, take an upvalue from
    /// their "ancestor" or leave a sentinel value on the stack.
    ///
    /// As such an upvalue is actually accessed, an error is produced
    /// when the sentinel is found. See the runtime's handling of
    /// dynamic upvalues for details.
    fn resolve_dynamic_upvalue(
        &mut self,
        at: usize,
        name: &str,
        node: &rnix::ast::Ident,
    ) -> Option<UpvalueIdx> {
        if at == 0 {
            // There can not be any upvalue at the outermost context.
            return None;
        }

        if let Some((lowest_idx, _)) = self
            .contexts
            .iter()
            .enumerate()
            .find(|(_, c)| c.scope.has_with())
        {
            // An enclosing lambda context has dynamic values. Each
            // context in the chain from that point on now needs to
            // capture dynamic upvalues because we can not statically
            // know at which level the correct one is located.
            let name = SmolStr::new(name);
            let mut upvalue_idx = None;

            for idx in lowest_idx..=at {
                upvalue_idx = Some(self.add_upvalue(
                    idx,
                    node,
                    UpvalueKind::Dynamic {
                        name: name.clone(),
                        up: upvalue_idx,
                    },
                ));
            }

            // Return the outermost upvalue index (i.e. the one of the
            // current context).
            return upvalue_idx;
        }

        None
    }

    fn add_upvalue(
        &mut self,
        ctx_idx: usize,
        node: &rnix::ast::Ident,
        kind: UpvalueKind,
    ) -> UpvalueIdx {
        // If there is already an upvalue closing over the specified
        // index, retrieve that instead.
        for (idx, existing) in self.contexts[ctx_idx].scope.upvalues.iter().enumerate() {
            if existing.kind == kind {
                return UpvalueIdx(idx);
            }
        }

        self.contexts[ctx_idx].scope.upvalues.push(Upvalue {
            kind,
            node: node.clone(),
        });

        let idx = UpvalueIdx(self.contexts[ctx_idx].lambda.upvalue_count);
        self.contexts[ctx_idx].lambda.upvalue_count += 1;
        idx
    }

    fn emit_force<N: AstNode>(&mut self, node: &N) {
        self.push_op(OpCode::OpForce, node);
    }

    fn emit_warning(&mut self, span: codemap::Span, kind: WarningKind) {
        self.warnings.push(EvalWarning { kind, span })
    }

    fn emit_error(&mut self, span: codemap::Span, kind: ErrorKind) {
        self.errors.push(Error { kind, span })
    }

    /// Convert a non-dynamic string expression to a string if possible,
    /// or raise an error.
    fn expr_str_to_string(&self, expr: ast::Str) -> EvalResult<String> {
        if expr.normalized_parts().len() == 1 {
            if let ast::InterpolPart::Literal(s) = expr.normalized_parts().pop().unwrap() {
                return Ok(s);
            }
        }

        return Err(Error {
            kind: ErrorKind::DynamicKeyInLet(expr.syntax().clone()),
            span: self.span_for(&expr),
        });
    }

    /// Convert a single identifier path fragment to a string if possible,
    /// or raise an error about the node being dynamic.
    fn attr_to_string(&self, node: ast::Attr) -> EvalResult<String> {
        match node {
            ast::Attr::Ident(ident) => Ok(ident.ident_token().unwrap().text().into()),
            ast::Attr::Str(s) => self.expr_str_to_string(s),

            // The dynamic node type is just a wrapper. C++ Nix does not
            // care about the dynamic wrapper when determining whether the
            // node itself is dynamic, it depends solely on the expression
            // inside (i.e. `let ${"a"} = 1; in a` is valid).
            ast::Attr::Dynamic(ref dynamic) => match dynamic.expr().unwrap() {
                ast::Expr::Str(s) => self.expr_str_to_string(s),
                _ => Err(Error {
                    kind: ErrorKind::DynamicKeyInLet(node.syntax().clone()),
                    span: self.span_for(&node),
                }),
            },
        }
    }

    // Normalises identifier fragments into a single string vector for
    // `let`-expressions; fails if fragments requiring dynamic computation
    // are encountered.
    fn normalise_ident_path<I: Iterator<Item = ast::Attr>>(
        &self,
        path: I,
    ) -> EvalResult<Vec<String>> {
        path.map(|node| self.attr_to_string(node)).collect()
    }
}

/// Prepare the full set of globals from additional globals supplied
/// by the caller of the compiler, as well as the built-in globals
/// that are always part of the language.
///
/// Note that all builtin functions are *not* considered part of the
/// language in this sense and MUST be supplied as additional global
/// values, including the `builtins` set itself.
fn prepare_globals(additional: HashMap<&'static str, Value>) -> GlobalsMap {
    let mut globals: GlobalsMap = HashMap::new();

    globals.insert(
        "true",
        Rc::new(|compiler, node| {
            compiler.push_op(OpCode::OpTrue, &node);
        }),
    );

    globals.insert(
        "false",
        Rc::new(|compiler, node| {
            compiler.push_op(OpCode::OpFalse, &node);
        }),
    );

    globals.insert(
        "null",
        Rc::new(|compiler, node| {
            compiler.push_op(OpCode::OpNull, &node);
        }),
    );

    for (ident, value) in additional.into_iter() {
        globals.insert(
            ident,
            Rc::new(move |compiler, node| compiler.emit_constant(value.clone(), &node)),
        );
    }

    globals
}

pub fn compile<'code>(
    expr: ast::Expr,
    location: Option<PathBuf>,
    file: &'code codemap::File,
    globals: HashMap<&'static str, Value>,

    #[cfg(feature = "disassembler")] codemap: Rc<codemap::CodeMap>,
) -> EvalResult<CompilationOutput> {
    let mut root_dir = match location {
        Some(dir) => Ok(dir),
        None => std::env::current_dir().map_err(|e| Error {
            kind: ErrorKind::PathResolution(format!(
                "could not determine current directory: {}",
                e
            )),
            span: file.span,
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
        file,
        #[cfg(feature = "disassembler")]
        codemap,
        globals: prepare_globals(globals),
        contexts: vec![LambdaCtx::new()],
        warnings: vec![],
        errors: vec![],
    };

    #[cfg(feature = "disassembler")]
    {
        c.context_mut().lambda.chunk.codemap = c.codemap.clone();
    }

    c.compile(LocalIdx::ZERO, expr.clone());

    // The final operation of any top-level Nix program must always be
    // `OpForce`. A thunk should not be returned to the user in an
    // unevaluated state (though in practice, a value *containing* a
    // thunk might be returned).
    c.emit_force(&expr);

    Ok(CompilationOutput {
        lambda: c.contexts.pop().unwrap().lambda,
        warnings: c.warnings,
        errors: c.errors,
    })
}
