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

mod bindings;
mod scope;
mod spans;

use codemap::Span;
use path_clean::PathClean;
use rnix::ast::{self, AstToken};
use smol_str::SmolStr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::chunk::Chunk;
use crate::errors::{Error, ErrorKind, EvalResult};
use crate::observer::Observer;
use crate::opcode::{CodeIdx, Count, JumpOffset, OpCode, UpvalueIdx};
use crate::value::{Closure, Lambda, Thunk, Value};
use crate::warnings::{EvalWarning, WarningKind};

use self::scope::{LocalIdx, LocalPosition, Scope, Upvalue, UpvalueKind};
use self::spans::ToSpan;

/// Represents the result of compiling a piece of Nix code. If
/// compilation was successful, the resulting bytecode can be passed
/// to the VM.
pub struct CompilationOutput {
    pub lambda: Rc<Lambda>,
    pub warnings: Vec<EvalWarning>,
    pub errors: Vec<Error>,
}

/// Represents the lambda currently being compiled.
struct LambdaCtx {
    lambda: Lambda,
    scope: Scope,
    captures_with_stack: bool,
}

impl LambdaCtx {
    fn new() -> Self {
        LambdaCtx {
            lambda: Lambda::new_anonymous(),
            scope: Default::default(),
            captures_with_stack: false,
        }
    }

    fn inherit(&self) -> Self {
        LambdaCtx {
            lambda: Lambda::new_anonymous(),
            scope: self.scope.inherit(),
            captures_with_stack: false,
        }
    }
}

/// Alias for the map of globally available functions that should
/// implicitly be resolvable in the global scope.
type GlobalsMap = HashMap<&'static str, Rc<dyn Fn(&mut Compiler, Span)>>;

struct Compiler<'observer> {
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
    file: Arc<codemap::File>,

    /// Carry an observer for the compilation process, which is called
    /// whenever a chunk is emitted.
    observer: &'observer mut dyn Observer,
}

/// Compiler construction
impl<'observer> Compiler<'observer> {
    pub(crate) fn new(
        location: Option<PathBuf>,
        file: Arc<codemap::File>,
        globals: HashMap<&'static str, Value>,
        observer: &'observer mut dyn Observer,
    ) -> EvalResult<Self> {
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

        Ok(Self {
            root_dir,
            file,
            observer,
            globals: prepare_globals(globals),
            contexts: vec![LambdaCtx::new()],
            warnings: vec![],
            errors: vec![],
        })
    }
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

    /// Push a single instruction to the current bytecode chunk and
    /// track the source span from which it was compiled.
    fn push_op<T: ToSpan>(&mut self, data: OpCode, node: &T) -> CodeIdx {
        let span = self.span_for(node);
        self.chunk().push_op(data, span)
    }

    /// Emit a single constant to the current bytecode chunk and track
    /// the source span from which it was compiled.
    fn emit_constant<T: ToSpan>(&mut self, value: Value, node: &T) {
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

            ast::Expr::BinOp(binop) => {
                self.thunk(slot, &binop, move |c, o, s| c.compile_binop(s, o.clone()))
            }

            ast::Expr::HasAttr(has_attr) => self.compile_has_attr(slot, has_attr),

            ast::Expr::List(list) => {
                self.thunk(slot, &list, move |c, l, s| c.compile_list(s, l.clone()))
            }

            ast::Expr::AttrSet(attrs) => self.thunk(slot, &attrs, move |c, a, s| {
                c.compile_attr_set(s, a.clone())
            }),

            ast::Expr::Select(select) => self.thunk(slot, &select, move |c, sel, s| {
                c.compile_select(s, sel.clone())
            }),

            ast::Expr::Assert(assert) => {
                self.thunk(slot, &assert, move |c, a, s| c.compile_assert(s, a.clone()))
            }
            ast::Expr::IfElse(if_else) => self.compile_if_else(slot, if_else),
            ast::Expr::LetIn(let_in) => self.compile_let_in(slot, let_in),
            ast::Expr::Ident(ident) => self.compile_ident(slot, ident),
            ast::Expr::With(with) => {
                self.thunk(slot, &with, |c, w, s| c.compile_with(s, w.clone()))
            }
            ast::Expr::Lambda(lambda) => self.compile_lambda(slot, lambda),
            ast::Expr::Apply(apply) => {
                self.thunk(slot, &apply, move |c, a, s| c.compile_apply(s, a.clone()))
            }

            // Parenthesized expressions are simply unwrapped, leaving
            // their value on the stack.
            ast::Expr::Paren(paren) => self.compile(slot, paren.expr().unwrap()),

            ast::Expr::LegacyLet(legacy_let) => self.compile_legacy_let(slot, legacy_let),

            ast::Expr::Root(_) => unreachable!("there cannot be more than one root"),
            ast::Expr::Error(_) => unreachable!("compile is only called on validated trees"),
        }
    }

    fn compile_literal(&mut self, node: ast::Literal) {
        let value = match node.kind() {
            ast::LiteralKind::Float(f) => Value::Float(f.value().unwrap()),
            ast::LiteralKind::Integer(i) => match i.value() {
                Ok(v) => Value::Integer(v),
                Err(err) => return self.emit_error(&node, err.into()),
            },

            ast::LiteralKind::Uri(u) => {
                self.emit_warning(&node, WarningKind::DeprecatedLiteralURL);
                Value::String(u.syntax().text().into())
            }
        };

        self.emit_constant(value, &node);
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
                        &node,
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
            self.emit_error(
                &node,
                ErrorKind::NotImplemented(
                    "other path types (e.g. <...> lookups) not yet implemented",
                ),
            );
            return;
        };

        // TODO: Use https://github.com/rust-lang/rfcs/issues/2208
        // once it is available
        let value = Value::Path(path.clean());
        self.emit_constant(value, &node);
    }

    /// Helper that compiles the given string parts strictly. The caller
    /// (`compile_str`) needs to figure out if the result of compiling this
    /// needs to be thunked or not.
    fn compile_str_parts(
        &mut self,
        slot: LocalIdx,
        parent_node: &ast::Str,
        parts: Vec<ast::InterpolPart<String>>,
    ) {
        // The string parts are produced in literal order, however
        // they need to be reversed on the stack in order to
        // efficiently create the real string in case of
        // interpolation.
        for part in parts.iter().rev() {
            match part {
                // Interpolated expressions are compiled as normal and
                // dealt with by the VM before being assembled into
                // the final string. We need to coerce them here,
                // so OpInterpolate definitely has a string to consume.
                ast::InterpolPart::Interpolation(ipol) => {
                    self.compile(slot, ipol.expr().unwrap());
                    // implicitly forces as well
                    self.push_op(OpCode::OpCoerceToString, ipol);
                }

                ast::InterpolPart::Literal(lit) => {
                    self.emit_constant(Value::String(lit.as_str().into()), parent_node);
                }
            }
        }

        if parts.len() != 1 {
            self.push_op(OpCode::OpInterpolate(Count(parts.len())), parent_node);
        }
    }

    fn compile_str(&mut self, slot: LocalIdx, node: ast::Str) {
        let parts = node.normalized_parts();

        // We need to thunk string expressions if they are the result of
        // interpolation. A string that only consists of a single part (`"${foo}"`)
        // can't desugar to the enclosed expression (`foo`) because we need to
        // coerce the result to a string value. This would require forcing the
        // value of the inner expression, so we need to wrap it in another thunk.
        if parts.len() != 1 || matches!(&parts[0], ast::InterpolPart::Interpolation(_)) {
            self.thunk(slot, &node, move |c, n, s| {
                c.compile_str_parts(s, n, parts);
            });
        } else {
            self.compile_str_parts(slot, &node, parts);
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

    /// Compile list literals into equivalent bytecode. List
    /// construction is fairly simple, consisting of pushing code for
    /// each literal element and an instruction with the element
    /// count.
    ///
    /// The VM, after evaluating the code for each element, simply
    /// constructs the list from the given number of elements.
    fn compile_list(&mut self, slot: LocalIdx, node: ast::List) {
        let mut count = 0;

        // Open a temporary scope to correctly account for stack items
        // that exist during the construction.
        self.scope_mut().begin_scope();

        for item in node.items() {
            // Start tracing new stack slots from the second list
            // element onwards. The first list element is located in
            // the stack slot of the list itself.
            let item_slot = match count {
                0 => slot,
                _ => {
                    let item_span = self.span_for(&item);
                    self.scope_mut().declare_phantom(item_span, false)
                }
            };

            count += 1;
            self.compile(item_slot, item);
            self.scope_mut().mark_initialised(item_slot);
        }

        self.push_op(OpCode::OpList(Count(count)), &node);
        self.scope_mut().end_scope();
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

    fn compile_has_attr(&mut self, slot: LocalIdx, node: ast::HasAttr) {
        // Put the attribute set on the stack.
        self.compile(slot, node.expr().unwrap());
        self.emit_force(&node);

        // Push all path fragments with an operation for fetching the
        // next nested element, for all fragments except the last one.
        for (count, fragment) in node.attrpath().unwrap().attrs().enumerate() {
            if count > 0 {
                self.push_op(OpCode::OpAttrsTrySelect, &fragment);
                self.emit_force(&fragment);
            }

            self.compile_attr(slot, fragment);
        }

        // After the last fragment, emit the actual instruction that
        // leaves a boolean on the stack.
        self.push_op(OpCode::OpHasAttr, &node);
    }

    fn compile_select(&mut self, slot: LocalIdx, node: ast::Select) {
        let set = node.expr().unwrap();
        let path = node.attrpath().unwrap();

        if node.or_token().is_some() {
            self.compile_select_or(slot, set, path, node.default_expr().unwrap());
            return;
        }

        // Push the set onto the stack
        self.compile(slot, set);

        // Compile each key fragment and emit access instructions.
        //
        // TODO: multi-select instruction to avoid re-pushing attrs on
        // nested selects.
        for fragment in path.attrs() {
            // Force the current set value.
            self.emit_force(&fragment);

            self.compile_attr(slot, fragment.clone());
            self.push_op(OpCode::OpAttrsSelect, &fragment);
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
        self.compile(slot, set);
        let mut jumps = vec![];

        for fragment in path.attrs() {
            self.emit_force(&fragment);
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
        self.emit_force(&node.condition().unwrap());
        self.push_op(OpCode::OpAssert, &node.condition().unwrap());

        // The runtime will abort evaluation at this point if the
        // assertion failed, if not the body simply continues on like
        // normal.
        self.compile(slot, node.body().unwrap());
    }

    /// Compile conditional expressions using jumping instructions in the VM.
    ///
    /// ```notrust
    ///                        ┌────────────────────┐
    ///                        │ 0  [ conditional ] │
    ///                        │ 1   JUMP_IF_FALSE →┼─┐
    ///                        │ 2  [  main body  ] │ │ Jump to else body if
    ///                       ┌┼─3─←     JUMP       │ │ condition is false.
    ///  Jump over else body  ││ 4  [  else body  ]←┼─┘
    ///  if condition is true.└┼─5─→     ...        │
    ///                        └────────────────────┘
    /// ```
    fn compile_if_else(&mut self, slot: LocalIdx, node: ast::IfElse) {
        self.compile(slot, node.condition().unwrap());
        self.emit_force(&node.condition().unwrap());

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

    /// Compile `with` expressions by emitting instructions that
    /// pop/remove the indices of attribute sets that are implicitly
    /// in scope through `with` on the "with-stack".
    fn compile_with(&mut self, slot: LocalIdx, node: ast::With) {
        self.scope_mut().begin_scope();
        // TODO: Detect if the namespace is just an identifier, and
        // resolve that directly (thus avoiding duplication on the
        // stack).
        self.compile(slot, node.namespace().unwrap());

        let span = self.span_for(&node.namespace().unwrap());

        // The attribute set from which `with` inherits values
        // occupies a slot on the stack, but this stack slot is not
        // directly accessible. As it must be accounted for to
        // calculate correct offsets, what we call a "phantom" local
        // is declared here.
        let local_idx = self.scope_mut().declare_phantom(span, true);
        let with_idx = self.scope().stack_index(local_idx);

        self.scope_mut().push_with();

        self.push_op(OpCode::OpPushWith(with_idx), &node.namespace().unwrap());

        self.compile(slot, node.body().unwrap());

        self.push_op(OpCode::OpPopWith, &node);
        self.scope_mut().pop_with();
        self.cleanup_scope(&node);
    }

    /// Compiles pattern function arguments, such as `{ a, b }: ...`.
    ///
    /// These patterns are treated as a special case of locals binding
    /// where the attribute set itself is placed on the first stack
    /// slot of the call frame (either as a phantom, or named in case
    /// of an `@` binding), and the function call sets up the rest of
    /// the stack as if the parameters were rewritten into a `let`
    /// binding.
    ///
    /// For example:
    ///
    /// ```nix
    /// ({ a, b ? 2, c ? a * b, ... }@args: <body>)  { a = 10; }
    /// ```
    ///
    /// would be compiled similarly to a binding such as
    ///
    /// ```nix
    /// let args = { a = 10; };
    /// in let a = args.a;
    ///        b = args.a or 2;
    ///        c = args.c or a * b;
    ///    in <body>
    /// ```
    ///
    /// The only tricky bit being that bindings have to fail if too
    /// many arguments are provided. This is done by emitting a
    /// special instruction that checks the set of keys from a
    /// constant containing the expected keys.
    fn compile_param_pattern(&mut self, pattern: ast::Pattern) {
        let span = self.span_for(&pattern);
        let set_idx = match pattern.pat_bind() {
            Some(name) => self.declare_local(&name, name.ident().unwrap().to_string()),
            None => self.scope_mut().declare_phantom(span, true),
        };

        // At call time, the attribute set is already at the top of
        // the stack.
        self.scope_mut().mark_initialised(set_idx);
        self.emit_force(&pattern);

        // Similar to `let ... in ...`, we now do multiple passes over
        // the bindings to first declare them, then populate them, and
        // then finalise any necessary recursion into the scope.
        let mut entries: Vec<(LocalIdx, ast::PatEntry)> = vec![];
        let mut indices: Vec<LocalIdx> = vec![];

        for entry in pattern.pat_entries() {
            let ident = entry.ident().unwrap();
            let idx = self.declare_local(&ident, ident.to_string());
            entries.push((idx, entry));
            indices.push(idx);
        }

        // For each of the bindings, push the set on the stack and
        // attempt to select from it.
        let stack_idx = self.scope().stack_index(set_idx);
        for (idx, entry) in entries.into_iter() {
            self.push_op(OpCode::OpGetLocal(stack_idx), &pattern);
            self.emit_literal_ident(&entry.ident().unwrap());

            // Use the same mechanism as `compile_select_or` if a
            // default value was provided, or simply select otherwise.
            if let Some(default_expr) = entry.default() {
                self.push_op(OpCode::OpAttrsTrySelect, &entry.ident().unwrap());

                let jump_to_default =
                    self.push_op(OpCode::OpJumpIfNotFound(JumpOffset(0)), &default_expr);

                let jump_over_default = self.push_op(OpCode::OpJump(JumpOffset(0)), &default_expr);

                self.patch_jump(jump_to_default);
                self.compile(idx, default_expr);
                self.patch_jump(jump_over_default);
            } else {
                self.push_op(OpCode::OpAttrsSelect, &entry.ident().unwrap());
            }

            self.scope_mut().mark_initialised(idx);
        }

        for idx in indices {
            if self.scope()[idx].needs_finaliser {
                let stack_idx = self.scope().stack_index(idx);
                self.push_op(OpCode::OpFinalise(stack_idx), &pattern);
            }
        }

        // TODO: strictly check if all keys have been consumed if
        // there is no ellipsis.
        if pattern.ellipsis_token().is_none() {
            self.emit_warning(&pattern, WarningKind::NotImplemented("closed formals"));
        }
    }

    fn compile_lambda(&mut self, outer_slot: LocalIdx, node: ast::Lambda) {
        self.new_context();
        let span = self.span_for(&node);
        let slot = self.scope_mut().declare_phantom(span, false);
        self.scope_mut().begin_scope();

        // Compile the function itself
        match node.param().unwrap() {
            ast::Param::Pattern(pat) => self.compile_param_pattern(pat),

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
        self.cleanup_scope(&node);

        // TODO: determine and insert enclosing name, if available.

        // Pop the lambda context back off, and emit the finished
        // lambda as a constant.
        let mut compiled = self.contexts.pop().unwrap();

        // Check if tail-call optimisation is possible and perform it.
        optimise_tail_call(&mut compiled.lambda.chunk);

        // Capturing the with stack counts as an upvalue, as it is
        // emitted as an upvalue data instruction.
        if compiled.captures_with_stack {
            compiled.lambda.upvalue_count += 1;
        }

        let lambda = Rc::new(compiled.lambda);
        self.observer.observe_compiled_lambda(&lambda);

        // If the function is not a closure, just emit it directly and
        // move on.
        if lambda.upvalue_count == 0 {
            self.emit_constant(Value::Closure(Closure::new(lambda)), &node);
            return;
        }

        // If the function is a closure, we need to emit the variable
        // number of operands that allow the runtime to close over the
        // upvalues and leave a blueprint in the constant index from
        // which the runtime closure can be constructed.
        let blueprint_idx = self.chunk().push_constant(Value::Blueprint(lambda));

        self.push_op(OpCode::OpClosure(blueprint_idx), &node);
        self.emit_upvalue_data(
            outer_slot,
            &node,
            compiled.scope.upvalues,
            compiled.captures_with_stack,
        );
    }

    fn compile_apply(&mut self, slot: LocalIdx, node: ast::Apply) {
        // To call a function, we leave its arguments on the stack,
        // followed by the function expression itself, and then emit a
        // call instruction. This way, the stack is perfectly laid out
        // to enter the function call straight away.
        self.compile(slot, node.argument().unwrap());
        self.compile(slot, node.lambda().unwrap());
        self.emit_force(&node.lambda().unwrap());
        self.push_op(OpCode::OpCall, &node);
    }

    /// Compile an expression into a runtime thunk which should be
    /// lazily evaluated when accessed.
    // TODO: almost the same as Compiler::compile_lambda; unify?
    fn thunk<N, F>(&mut self, outer_slot: LocalIdx, node: &N, content: F)
    where
        N: ToSpan + Clone,
        F: FnOnce(&mut Compiler, &N, LocalIdx),
    {
        self.new_context();
        let span = self.span_for(node);
        let slot = self.scope_mut().declare_phantom(span, false);
        self.scope_mut().begin_scope();
        content(self, node, slot);
        self.cleanup_scope(node);

        let mut thunk = self.contexts.pop().unwrap();
        optimise_tail_call(&mut thunk.lambda.chunk);

        // Capturing the with stack counts as an upvalue, as it is
        // emitted as an upvalue data instruction.
        if thunk.captures_with_stack {
            thunk.lambda.upvalue_count += 1;
        }

        let lambda = Rc::new(thunk.lambda);
        self.observer.observe_compiled_thunk(&lambda);

        // Emit the thunk directly if it does not close over the
        // environment.
        if lambda.upvalue_count == 0 {
            self.emit_constant(Value::Thunk(Thunk::new(lambda)), node);
            return;
        }

        // Otherwise prepare for runtime construction of the thunk.
        let blueprint_idx = self.chunk().push_constant(Value::Blueprint(lambda));

        self.push_op(OpCode::OpThunk(blueprint_idx), node);
        self.emit_upvalue_data(
            outer_slot,
            node,
            thunk.scope.upvalues,
            thunk.captures_with_stack,
        );
    }

    /// Emit the data instructions that the runtime needs to correctly
    /// assemble the upvalues struct.
    fn emit_upvalue_data<T: ToSpan>(
        &mut self,
        slot: LocalIdx,
        node: &T,
        upvalues: Vec<Upvalue>,
        capture_with: bool,
    ) {
        let this_depth = self.scope()[slot].depth;
        let this_stack_slot = self.scope().stack_index(slot);

        for upvalue in upvalues {
            match upvalue.kind {
                UpvalueKind::Local(idx) => {
                    let target_depth = self.scope()[idx].depth;
                    let stack_idx = self.scope().stack_index(idx);

                    // If the upvalue slot is located at the same
                    // depth, but *after* the closure, the upvalue
                    // resolution must be deferred until the scope is
                    // fully initialised and can be finalised.
                    if this_depth == target_depth && this_stack_slot < stack_idx {
                        self.push_op(OpCode::DataDeferredLocal(stack_idx), &upvalue.span);
                        self.scope_mut().mark_needs_finaliser(slot);
                    } else {
                        self.push_op(OpCode::DataLocalIdx(stack_idx), &upvalue.span);
                    }
                }

                UpvalueKind::Upvalue(idx) => {
                    self.push_op(OpCode::DataUpvalueIdx(idx), &upvalue.span);
                }
            };
        }

        if capture_with {
            // TODO(tazjin): probably better to emit span for the ident that caused this
            self.push_op(OpCode::DataCaptureWith, node);
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

    /// Decrease scope depth of the current function and emit
    /// instructions to clean up the stack at runtime.
    fn cleanup_scope<N: ToSpan>(&mut self, node: &N) {
        // When ending a scope, all corresponding locals need to be
        // removed, but the value of the body needs to remain on the
        // stack. This is implemented by a separate instruction.
        let (popcount, unused_spans) = self.scope_mut().end_scope();

        for span in &unused_spans {
            self.emit_warning(span, WarningKind::UnusedBinding);
        }

        if popcount > 0 {
            self.push_op(OpCode::OpCloseScope(Count(popcount)), node);
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
    fn declare_local<S: Into<String>, N: ToSpan>(&mut self, node: &N, name: S) -> LocalIdx {
        let name = name.into();
        let depth = self.scope().scope_depth();

        // Do this little dance to get ahold of the *static* key and
        // use it for poisoning if required.
        let key: Option<&'static str> = match self.globals.get_key_value(name.as_str()) {
            Some((key, _)) => Some(*key),
            None => None,
        };

        if let Some(global_ident) = key {
            self.emit_warning(node, WarningKind::ShadowedGlobal(global_ident));
            self.scope_mut().poison(global_ident, depth);
        }

        for other in self.scope().locals.iter().rev() {
            if other.has_name(&name) && other.depth == depth {
                self.emit_error(node, ErrorKind::VariableAlreadyDefined(other.span));

                break;
            }
        }

        let span = self.span_for(node);
        self.scope_mut().declare_local(name, span)
    }

    /// Determine whether the current lambda context has any ancestors
    /// that use dynamic scope resolution, and mark contexts as
    /// needing to capture their enclosing `with`-stack in their
    /// upvalues.
    fn has_dynamic_ancestor(&mut self) -> bool {
        let mut ancestor_has_with = false;

        for ctx in self.contexts.iter_mut() {
            if ancestor_has_with {
                // If the ancestor has an active with stack, mark this
                // lambda context as needing to capture it.
                ctx.captures_with_stack = true;
            } else {
                // otherwise, check this context and move on
                ancestor_has_with = ctx.scope.has_with();
            }
        }

        ancestor_has_with
    }

    fn emit_force<N: ToSpan>(&mut self, node: &N) {
        self.push_op(OpCode::OpForce, node);
    }

    fn emit_warning<N: ToSpan>(&mut self, node: &N, kind: WarningKind) {
        let span = self.span_for(node);
        self.warnings.push(EvalWarning { kind, span })
    }

    fn emit_error<N: ToSpan>(&mut self, node: &N, kind: ErrorKind) {
        let span = self.span_for(node);
        self.errors.push(Error { kind, span })
    }
}

/// Perform tail-call optimisation if the last call within a
/// compiled chunk is another call.
fn optimise_tail_call(chunk: &mut Chunk) {
    let last_op = chunk
        .code
        .last_mut()
        .expect("compiler bug: chunk should never be empty");

    if matches!(last_op, OpCode::OpCall) {
        *last_op = OpCode::OpTailCall;
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
        Rc::new(|compiler, span| {
            compiler.push_op(OpCode::OpTrue, &span);
        }),
    );

    globals.insert(
        "false",
        Rc::new(|compiler, span| {
            compiler.push_op(OpCode::OpFalse, &span);
        }),
    );

    globals.insert(
        "null",
        Rc::new(|compiler, span| {
            compiler.push_op(OpCode::OpNull, &span);
        }),
    );

    for (ident, value) in additional.into_iter() {
        globals.insert(
            ident,
            Rc::new(move |compiler, span| compiler.emit_constant(value.clone(), &span)),
        );
    }

    globals
}

pub fn compile(
    expr: ast::Expr,
    location: Option<PathBuf>,
    file: Arc<codemap::File>,
    globals: HashMap<&'static str, Value>,
    observer: &mut dyn Observer,
) -> EvalResult<CompilationOutput> {
    let mut c = Compiler::new(location, file, globals, observer)?;

    let root_span = c.span_for(&expr);
    let root_slot = c.scope_mut().declare_phantom(root_span, false);
    c.compile(root_slot, expr.clone());

    // The final operation of any top-level Nix program must always be
    // `OpForce`. A thunk should not be returned to the user in an
    // unevaluated state (though in practice, a value *containing* a
    // thunk might be returned).
    c.emit_force(&expr);

    let lambda = Rc::new(c.contexts.pop().unwrap().lambda);
    c.observer.observe_compiled_toplevel(&lambda);

    Ok(CompilationOutput {
        lambda,
        warnings: c.warnings,
        errors: c.errors,
    })
}
