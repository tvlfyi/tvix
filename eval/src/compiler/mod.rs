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
mod import;
mod optimiser;
mod scope;

use codemap::Span;
use rnix::ast::{self, AstToken};
use smol_str::SmolStr;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use crate::chunk::Chunk;
use crate::errors::{Error, ErrorKind, EvalResult};
use crate::observer::CompilerObserver;
use crate::opcode::{CodeIdx, ConstantIdx, Count, JumpOffset, OpCode, UpvalueIdx};
use crate::spans::LightSpan;
use crate::spans::ToSpan;
use crate::value::{Closure, Formals, Lambda, NixAttrs, Thunk, Value};
use crate::warnings::{EvalWarning, WarningKind};
use crate::SourceCode;

use self::scope::{LocalIdx, LocalPosition, Scope, Upvalue, UpvalueKind};

/// Represents the result of compiling a piece of Nix code. If
/// compilation was successful, the resulting bytecode can be passed
/// to the VM.
pub struct CompilationOutput {
    pub lambda: Rc<Lambda>,
    pub warnings: Vec<EvalWarning>,
    pub errors: Vec<Error>,

    // This field must outlive the rc::Weak reference which breaks the
    // builtins -> import -> builtins reference cycle. For this
    // reason, it must be passed to the VM.
    pub globals: Rc<GlobalsMap>,
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
            lambda: Lambda::default(),
            scope: Default::default(),
            captures_with_stack: false,
        }
    }

    fn inherit(&self) -> Self {
        LambdaCtx {
            lambda: Lambda::default(),
            scope: self.scope.inherit(),
            captures_with_stack: false,
        }
    }
}

/// The map of globally available functions and other values that
/// should implicitly be resolvable in the global scope.
pub(crate) type GlobalsMap = HashMap<&'static str, Value>;

/// Set of builtins that (if they exist) should be made available in
/// the global scope, meaning that they can be accessed not just
/// through `builtins.<name>`, but directly as `<name>`. This is not
/// configurable, it is based on what Nix 2.3 exposed.
const GLOBAL_BUILTINS: &[&str] = &[
    "abort",
    "baseNameOf",
    "derivation",
    "derivationStrict",
    "dirOf",
    "fetchGit",
    "fetchMercurial",
    "fetchTarball",
    "fromTOML",
    "import",
    "isNull",
    "map",
    "placeholder",
    "removeAttrs",
    "scopedImport",
    "throw",
    "toString",
];

pub struct Compiler<'observer> {
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
    globals: Rc<GlobalsMap>,

    /// File reference in the codemap contains all known source code
    /// and is used to track the spans from which instructions where
    /// derived.
    file: Arc<codemap::File>,

    /// Carry an observer for the compilation process, which is called
    /// whenever a chunk is emitted.
    observer: &'observer mut dyn CompilerObserver,

    /// Carry a count of nested scopes which have requested the
    /// compiler not to emit anything. This used for compiling dead
    /// code branches to catch errors & warnings in them.
    dead_scope: usize,
}

impl Compiler<'_> {
    pub(super) fn span_for<S: ToSpan>(&self, to_span: &S) -> Span {
        to_span.span_for(&self.file)
    }
}

/// Compiler construction
impl<'observer> Compiler<'observer> {
    pub(crate) fn new(
        location: Option<PathBuf>,
        file: Arc<codemap::File>,
        globals: Rc<GlobalsMap>,
        observer: &'observer mut dyn CompilerObserver,
    ) -> EvalResult<Self> {
        let mut root_dir = match location {
            Some(dir) if cfg!(target_arch = "wasm32") || dir.is_absolute() => Ok(dir),
            _ => {
                let current_dir = std::env::current_dir().map_err(|e| {
                    Error::new(
                        ErrorKind::RelativePathResolution(format!(
                            "could not determine current directory: {}",
                            e
                        )),
                        file.span,
                    )
                })?;
                if let Some(dir) = location {
                    Ok(current_dir.join(dir))
                } else {
                    Ok(current_dir)
                }
            }
        }?;

        // If the path passed from the caller points to a file, the
        // filename itself needs to be truncated as this must point to a
        // directory.
        if root_dir.is_file() {
            root_dir.pop();
        }

        #[cfg(not(target_arch = "wasm32"))]
        debug_assert!(root_dir.is_absolute());

        Ok(Self {
            root_dir,
            file,
            observer,
            globals,
            contexts: vec![LambdaCtx::new()],
            warnings: vec![],
            errors: vec![],
            dead_scope: 0,
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
        if self.dead_scope > 0 {
            return CodeIdx(0);
        }

        let span = self.span_for(node);
        self.chunk().push_op(data, span)
    }

    /// Emit a single constant to the current bytecode chunk and track
    /// the source span from which it was compiled.
    pub(super) fn emit_constant<T: ToSpan>(&mut self, value: Value, node: &T) {
        if self.dead_scope > 0 {
            return;
        }

        let idx = self.chunk().push_constant(value);
        self.push_op(OpCode::OpConstant(idx), node);
    }
}

// Actual code-emitting AST traversal methods.
impl Compiler<'_> {
    fn compile(&mut self, slot: LocalIdx, expr: ast::Expr) {
        let expr = optimiser::optimise_expr(self, slot, expr);

        match &expr {
            ast::Expr::Literal(literal) => self.compile_literal(literal),
            ast::Expr::Path(path) => self.compile_path(slot, path),
            ast::Expr::Str(s) => self.compile_str(slot, s),

            ast::Expr::UnaryOp(op) => self.compile_unary_op(slot, op),

            ast::Expr::BinOp(binop) => {
                self.thunk(slot, binop, move |c, s| c.compile_binop(s, binop))
            }

            ast::Expr::HasAttr(has_attr) => self.compile_has_attr(slot, has_attr),

            ast::Expr::List(list) => self.thunk(slot, list, move |c, s| c.compile_list(s, list)),

            ast::Expr::AttrSet(attrs) => {
                self.thunk(slot, attrs, move |c, s| c.compile_attr_set(s, attrs))
            }

            ast::Expr::Select(select) => {
                self.thunk(slot, select, move |c, s| c.compile_select(s, select))
            }

            ast::Expr::Assert(assert) => {
                self.thunk(slot, assert, move |c, s| c.compile_assert(s, assert))
            }
            ast::Expr::IfElse(if_else) => {
                self.thunk(slot, if_else, move |c, s| c.compile_if_else(s, if_else))
            }

            ast::Expr::LetIn(let_in) => {
                self.thunk(slot, let_in, move |c, s| c.compile_let_in(s, let_in))
            }

            ast::Expr::Ident(ident) => self.compile_ident(slot, ident),
            ast::Expr::With(with) => self.thunk(slot, with, |c, s| c.compile_with(s, with)),
            ast::Expr::Lambda(lambda) => {
                self.compile_lambda_or_thunk(false, slot, lambda, |c, s| {
                    c.compile_lambda(s, lambda)
                })
            }
            ast::Expr::Apply(apply) => {
                self.thunk(slot, apply, move |c, s| c.compile_apply(s, apply))
            }

            // Parenthesized expressions are simply unwrapped, leaving
            // their value on the stack.
            ast::Expr::Paren(paren) => self.compile(slot, paren.expr().unwrap()),

            ast::Expr::LegacyLet(legacy_let) => self.compile_legacy_let(slot, legacy_let),

            ast::Expr::Root(_) => unreachable!("there cannot be more than one root"),
            ast::Expr::Error(_) => unreachable!("compile is only called on validated trees"),
        }
    }

    /// Compiles an expression, but does not emit any code for it as
    /// it is considered dead. This will still catch errors and
    /// warnings in that expression.
    ///
    /// A warning about the that code being dead is assumed to already be
    /// emitted by the caller of [compile_dead_code].
    fn compile_dead_code(&mut self, slot: LocalIdx, node: ast::Expr) {
        self.dead_scope += 1;
        self.compile(slot, node);
        self.dead_scope -= 1;
    }

    fn compile_literal(&mut self, node: &ast::Literal) {
        let value = match node.kind() {
            ast::LiteralKind::Float(f) => Value::Float(f.value().unwrap()),
            ast::LiteralKind::Integer(i) => match i.value() {
                Ok(v) => Value::Integer(v),
                Err(err) => return self.emit_error(node, err.into()),
            },

            ast::LiteralKind::Uri(u) => {
                self.emit_warning(node, WarningKind::DeprecatedLiteralURL);
                Value::String(u.syntax().text().into())
            }
        };

        self.emit_constant(value, node);
    }

    fn compile_path(&mut self, slot: LocalIdx, node: &ast::Path) {
        // TODO(tazjin): placeholder implementation while waiting for
        // https://github.com/nix-community/rnix-parser/pull/96

        let raw_path = node.to_string();
        let path = if raw_path.starts_with('/') {
            Path::new(&raw_path).to_owned()
        } else if raw_path.starts_with('~') {
            return self.thunk(slot, node, move |c, _| {
                // We assume that home paths start with ~/ or fail to parse
                // TODO: this should be checked using a parse-fail test.
                debug_assert!(raw_path.len() > 2 && raw_path.starts_with("~/"));

                let home_relative_path = &raw_path[2..(raw_path.len())];
                c.emit_constant(
                    Value::UnresolvedPath(Box::new(home_relative_path.into())),
                    node,
                );
                c.push_op(OpCode::OpResolveHomePath, node);
            });
        } else if raw_path.starts_with('<') {
            // TODO: decide what to do with findFile
            if raw_path.len() == 2 {
                return self.emit_error(
                    node,
                    ErrorKind::NixPathResolution("Empty <> path not allowed".into()),
                );
            }
            let path = &raw_path[1..(raw_path.len() - 1)];
            // Make a thunk to resolve the path (without using `findFile`, at least for now?)
            return self.thunk(slot, node, move |c, _| {
                c.emit_constant(Value::UnresolvedPath(Box::new(path.into())), node);
                c.push_op(OpCode::OpFindFile, node);
            });
        } else {
            let mut buf = self.root_dir.clone();
            buf.push(&raw_path);
            buf
        };

        // TODO: Use https://github.com/rust-lang/rfcs/issues/2208
        // once it is available
        let value = Value::Path(Box::new(crate::value::canon_path(path)));
        self.emit_constant(value, node);
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

    fn compile_str(&mut self, slot: LocalIdx, node: &ast::Str) {
        let parts = node.normalized_parts();

        // We need to thunk string expressions if they are the result of
        // interpolation. A string that only consists of a single part (`"${foo}"`)
        // can't desugar to the enclosed expression (`foo`) because we need to
        // coerce the result to a string value. This would require forcing the
        // value of the inner expression, so we need to wrap it in another thunk.
        if parts.len() != 1 || matches!(&parts[0], ast::InterpolPart::Interpolation(_)) {
            self.thunk(slot, node, move |c, s| {
                c.compile_str_parts(s, node, parts);
            });
        } else {
            self.compile_str_parts(slot, node, parts);
        }
    }

    fn compile_unary_op(&mut self, slot: LocalIdx, op: &ast::UnaryOp) {
        self.compile(slot, op.expr().unwrap());
        self.emit_force(op);

        let opcode = match op.operator().unwrap() {
            ast::UnaryOpKind::Invert => OpCode::OpInvert,
            ast::UnaryOpKind::Negate => OpCode::OpNegate,
        };

        self.push_op(opcode, op);
    }

    fn compile_binop(&mut self, slot: LocalIdx, op: &ast::BinOp) {
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
            BinOpKind::Add => self.push_op(OpCode::OpAdd, op),
            BinOpKind::Sub => self.push_op(OpCode::OpSub, op),
            BinOpKind::Mul => self.push_op(OpCode::OpMul, op),
            BinOpKind::Div => self.push_op(OpCode::OpDiv, op),
            BinOpKind::Update => self.push_op(OpCode::OpAttrsUpdate, op),
            BinOpKind::Equal => self.push_op(OpCode::OpEqual, op),
            BinOpKind::Less => self.push_op(OpCode::OpLess, op),
            BinOpKind::LessOrEq => self.push_op(OpCode::OpLessOrEq, op),
            BinOpKind::More => self.push_op(OpCode::OpMore, op),
            BinOpKind::MoreOrEq => self.push_op(OpCode::OpMoreOrEq, op),
            BinOpKind::Concat => self.push_op(OpCode::OpConcat, op),

            BinOpKind::NotEqual => {
                self.push_op(OpCode::OpEqual, op);
                self.push_op(OpCode::OpInvert, op)
            }

            // Handled by separate branch above.
            BinOpKind::And | BinOpKind::Implication | BinOpKind::Or => {
                unreachable!()
            }
        };
    }

    fn compile_and(&mut self, slot: LocalIdx, node: &ast::BinOp) {
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
        let end_idx = self.push_op(OpCode::OpJumpIfFalse(JumpOffset(0)), node);

        // Otherwise, remove the previous value and leave the
        // right-hand side on the stack. Its result is now the value
        // of the whole expression.
        self.push_op(OpCode::OpPop, node);
        self.compile(slot, node.rhs().unwrap());
        self.emit_force(&node.rhs().unwrap());

        self.patch_jump(end_idx);
        self.push_op(OpCode::OpAssertBool, node);
    }

    fn compile_or(&mut self, slot: LocalIdx, node: &ast::BinOp) {
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
        let end_idx = self.push_op(OpCode::OpJumpIfTrue(JumpOffset(0)), node);
        self.push_op(OpCode::OpPop, node);
        self.compile(slot, node.rhs().unwrap());
        self.emit_force(&node.rhs().unwrap());

        self.patch_jump(end_idx);
        self.push_op(OpCode::OpAssertBool, node);
    }

    fn compile_implication(&mut self, slot: LocalIdx, node: &ast::BinOp) {
        debug_assert!(
            matches!(node.operator(), Some(ast::BinOpKind::Implication)),
            "compile_implication called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack and invert it.
        self.compile(slot, node.lhs().unwrap());
        self.emit_force(&node.lhs().unwrap());
        self.push_op(OpCode::OpInvert, node);

        // Exactly as `||` (because `a -> b` = `!a || b`).
        let end_idx = self.push_op(OpCode::OpJumpIfTrue(JumpOffset(0)), node);
        self.push_op(OpCode::OpPop, node);
        self.compile(slot, node.rhs().unwrap());
        self.emit_force(&node.rhs().unwrap());

        self.patch_jump(end_idx);
        self.push_op(OpCode::OpAssertBool, node);
    }

    /// Compile list literals into equivalent bytecode. List
    /// construction is fairly simple, consisting of pushing code for
    /// each literal element and an instruction with the element
    /// count.
    ///
    /// The VM, after evaluating the code for each element, simply
    /// constructs the list from the given number of elements.
    fn compile_list(&mut self, slot: LocalIdx, node: &ast::List) {
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

        self.push_op(OpCode::OpList(Count(count)), node);
        self.scope_mut().end_scope();
    }

    fn compile_attr(&mut self, slot: LocalIdx, node: &ast::Attr) {
        match node {
            ast::Attr::Dynamic(dynamic) => {
                self.compile(slot, dynamic.expr().unwrap());
                self.emit_force(&dynamic.expr().unwrap());
            }

            ast::Attr::Str(s) => {
                self.compile_str(slot, s);
                self.emit_force(s);
            }

            ast::Attr::Ident(ident) => self.emit_literal_ident(ident),
        }
    }

    fn compile_has_attr(&mut self, slot: LocalIdx, node: &ast::HasAttr) {
        // Put the attribute set on the stack.
        self.compile(slot, node.expr().unwrap());
        self.emit_force(node);

        // Push all path fragments with an operation for fetching the
        // next nested element, for all fragments except the last one.
        for (count, fragment) in node.attrpath().unwrap().attrs().enumerate() {
            if count > 0 {
                self.push_op(OpCode::OpAttrsTrySelect, &fragment);
                self.emit_force(&fragment);
            }

            self.compile_attr(slot, &fragment);
        }

        // After the last fragment, emit the actual instruction that
        // leaves a boolean on the stack.
        self.push_op(OpCode::OpHasAttr, node);
    }

    /// When compiling select or select_or expressions, an optimisation is
    /// possible of compiling the set emitted a constant attribute set by
    /// immediately replacing it with the actual value.
    ///
    /// We take care not to emit an error here, as that would interfere with
    /// thunking behaviour (there can be perfectly valid Nix code that accesses
    /// a statically known attribute set that is lacking a key, because that
    /// thunk is never evaluated). If anything is missing, just inform the
    /// caller that the optimisation did not take place and move on. We may want
    /// to emit warnings here in the future.
    fn optimise_select(&mut self, path: &ast::Attrpath) -> bool {
        // If compiling the set emitted a constant attribute set, the
        // associated constant can immediately be replaced with the
        // actual value.
        //
        // We take care not to emit an error here, as that would
        // interfere with thunking behaviour (there can be perfectly
        // valid Nix code that accesses a statically known attribute
        // set that is lacking a key, because that thunk is never
        // evaluated). If anything is missing, just move on. We may
        // want to emit warnings here in the future.
        if let Some(OpCode::OpConstant(ConstantIdx(idx))) = self.chunk().code.last().cloned() {
            let constant = &mut self.chunk().constants[idx];
            if let Value::Attrs(attrs) = constant {
                let mut path_iter = path.attrs();

                // Only do this optimisation if there is a *single*
                // element in the attribute path. It is extremely
                // unlikely that we'd have a static nested set.
                if let (Some(attr), None) = (path_iter.next(), path_iter.next()) {
                    // Only do this optimisation for statically known attrs.
                    if let Some(ident) = expr_static_attr_str(&attr) {
                        if let Some(selected_value) = attrs.select(ident.as_str()) {
                            *constant = selected_value.clone();
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    fn compile_select(&mut self, slot: LocalIdx, node: &ast::Select) {
        let set = node.expr().unwrap();
        let path = node.attrpath().unwrap();

        if node.or_token().is_some() {
            return self.compile_select_or(slot, set, path, node.default_expr().unwrap());
        }

        // Push the set onto the stack
        self.compile(slot, set);
        if self.optimise_select(&path) {
            return;
        }

        // Compile each key fragment and emit access instructions.
        //
        // TODO: multi-select instruction to avoid re-pushing attrs on
        // nested selects.
        for fragment in path.attrs() {
            // Force the current set value.
            self.emit_force(&fragment);

            self.compile_attr(slot, &fragment);
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
        if self.optimise_select(&path) {
            return;
        }

        let mut jumps = vec![];

        for fragment in path.attrs() {
            self.emit_force(&fragment);
            self.compile_attr(slot, &fragment.clone());
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

    /// Compile `assert` expressions using jumping instructions in the VM.
    ///
    /// ```notrust
    ///                        ┌─────────────────────┐
    ///                        │ 0  [ conditional ]  │
    ///                        │ 1   JUMP_IF_FALSE  →┼─┐
    ///                        │ 2  [  main body  ]  │ │ Jump to else body if
    ///                       ┌┼─3─←     JUMP        │ │ condition is false.
    ///  Jump over else body  ││ 4   OP_ASSERT_FAIL ←┼─┘
    ///  if condition is true.└┼─5─→     ...         │
    ///                        └─────────────────────┘
    /// ```
    fn compile_assert(&mut self, slot: LocalIdx, node: &ast::Assert) {
        // Compile the assertion condition to leave its value on the stack.
        self.compile(slot, node.condition().unwrap());
        self.emit_force(&node.condition().unwrap());
        let then_idx = self.push_op(OpCode::OpJumpIfFalse(JumpOffset(0)), node);

        self.push_op(OpCode::OpPop, node);
        self.compile(slot, node.body().unwrap());

        let else_idx = self.push_op(OpCode::OpJump(JumpOffset(0)), node);

        self.patch_jump(then_idx);
        self.push_op(OpCode::OpPop, node);
        self.push_op(OpCode::OpAssertFail, &node.condition().unwrap());

        self.patch_jump(else_idx);
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
    fn compile_if_else(&mut self, slot: LocalIdx, node: &ast::IfElse) {
        self.compile(slot, node.condition().unwrap());
        self.emit_force(&node.condition().unwrap());

        let then_idx = self.push_op(
            OpCode::OpJumpIfFalse(JumpOffset(0)),
            &node.condition().unwrap(),
        );

        self.push_op(OpCode::OpPop, node); // discard condition value
        self.compile(slot, node.body().unwrap());

        let else_idx = self.push_op(OpCode::OpJump(JumpOffset(0)), node);

        self.patch_jump(then_idx); // patch jump *to* else_body
        self.push_op(OpCode::OpPop, node); // discard condition value
        self.compile(slot, node.else_body().unwrap());

        self.patch_jump(else_idx); // patch jump *over* else body
    }

    /// Compile `with` expressions by emitting instructions that
    /// pop/remove the indices of attribute sets that are implicitly
    /// in scope through `with` on the "with-stack".
    fn compile_with(&mut self, slot: LocalIdx, node: &ast::With) {
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

        self.push_op(OpCode::OpPopWith, node);
        self.scope_mut().pop_with();
        self.cleanup_scope(node);
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
    fn compile_param_pattern(&mut self, pattern: &ast::Pattern) -> Formals {
        let span = self.span_for(pattern);
        let set_idx = match pattern.pat_bind() {
            Some(name) => self.declare_local(&name, name.ident().unwrap().to_string()),
            None => self.scope_mut().declare_phantom(span, true),
        };

        // At call time, the attribute set is already at the top of
        // the stack.
        self.scope_mut().mark_initialised(set_idx);
        self.emit_force(pattern);

        let ellipsis = pattern.ellipsis_token().is_some();
        if !ellipsis {
            self.push_op(OpCode::OpValidateClosedFormals, pattern);
        }

        // Similar to `let ... in ...`, we now do multiple passes over
        // the bindings to first declare them, then populate them, and
        // then finalise any necessary recursion into the scope.
        let mut entries: Vec<(LocalIdx, ast::PatEntry)> = vec![];
        let mut indices: Vec<LocalIdx> = vec![];
        let mut arguments = HashMap::default();

        for entry in pattern.pat_entries() {
            let ident = entry.ident().unwrap();
            let idx = self.declare_local(&ident, ident.to_string());
            let has_default = entry.default().is_some();
            entries.push((idx, entry));
            indices.push(idx);
            arguments.insert(ident.into(), has_default);
        }

        // For each of the bindings, push the set on the stack and
        // attempt to select from it.
        let stack_idx = self.scope().stack_index(set_idx);
        for (idx, entry) in entries.into_iter() {
            self.push_op(OpCode::OpGetLocal(stack_idx), pattern);
            self.emit_literal_ident(&entry.ident().unwrap());

            // Use the same mechanism as `compile_select_or` if a
            // default value was provided, or simply select otherwise.
            if let Some(default_expr) = entry.default() {
                self.push_op(OpCode::OpAttrsTrySelect, &entry.ident().unwrap());

                let jump_to_default =
                    self.push_op(OpCode::OpJumpIfNotFound(JumpOffset(0)), &default_expr);

                let jump_over_default = self.push_op(OpCode::OpJump(JumpOffset(0)), &default_expr);

                self.patch_jump(jump_to_default);

                // Thunk the default expression, but only if it is something
                // other than an identifier.
                if let ast::Expr::Ident(_) = &default_expr {
                    self.compile(idx, default_expr);
                } else {
                    self.thunk(idx, &self.span_for(&default_expr), move |c, s| {
                        c.compile(s, default_expr)
                    });
                }

                self.patch_jump(jump_over_default);
            } else {
                self.push_op(OpCode::OpAttrsSelect, &entry.ident().unwrap());
            }

            self.scope_mut().mark_initialised(idx);
        }

        for idx in indices {
            if self.scope()[idx].needs_finaliser {
                let stack_idx = self.scope().stack_index(idx);
                self.push_op(OpCode::OpFinalise(stack_idx), pattern);
            }
        }

        Formals {
            arguments,
            ellipsis,
            span,
        }
    }

    fn compile_lambda(&mut self, slot: LocalIdx, node: &ast::Lambda) {
        // Compile the function itself, recording its formal arguments (if any)
        // for later use
        let formals = match node.param().unwrap() {
            ast::Param::Pattern(pat) => Some(self.compile_param_pattern(&pat)),

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
                None
            }
        };

        self.compile(slot, node.body().unwrap());
        self.context_mut().lambda.formals = formals;
    }

    fn thunk<N, F>(&mut self, outer_slot: LocalIdx, node: &N, content: F)
    where
        N: ToSpan,
        F: FnOnce(&mut Compiler, LocalIdx),
    {
        self.compile_lambda_or_thunk(true, outer_slot, node, content)
    }

    /// Compile an expression into a runtime cloure or thunk
    fn compile_lambda_or_thunk<N, F>(
        &mut self,
        is_suspended_thunk: bool,
        outer_slot: LocalIdx,
        node: &N,
        content: F,
    ) where
        N: ToSpan,
        F: FnOnce(&mut Compiler, LocalIdx),
    {
        let name = self.scope()[outer_slot].name();
        self.new_context();

        // Set the (optional) name of the current slot on the lambda that is
        // being compiled.
        self.context_mut().lambda.name = name;

        let span = self.span_for(node);
        let slot = self.scope_mut().declare_phantom(span, false);
        self.scope_mut().begin_scope();

        content(self, slot);
        self.cleanup_scope(node);

        // TODO: determine and insert enclosing name, if available.

        // Pop the lambda context back off, and emit the finished
        // lambda as a constant.
        let mut compiled = self.contexts.pop().unwrap();

        // Emit an instruction to inform the VM that the chunk has ended.
        compiled
            .lambda
            .chunk
            .push_op(OpCode::OpReturn, self.span_for(node));

        // Capturing the with stack counts as an upvalue, as it is
        // emitted as an upvalue data instruction.
        if compiled.captures_with_stack {
            compiled.lambda.upvalue_count += 1;
        }

        let lambda = Rc::new(compiled.lambda);
        if is_suspended_thunk {
            self.observer.observe_compiled_thunk(&lambda);
        } else {
            self.observer.observe_compiled_lambda(&lambda);
        }

        // If no upvalues are captured, emit directly and move on.
        if lambda.upvalue_count == 0 {
            self.emit_constant(
                if is_suspended_thunk {
                    Value::Thunk(Thunk::new_suspended(lambda, LightSpan::new_actual(span)))
                } else {
                    Value::Closure(Rc::new(Closure::new(lambda)))
                },
                node,
            );
            return;
        }

        // Otherwise, we need to emit the variable number of
        // operands that allow the runtime to close over the
        // upvalues and leave a blueprint in the constant index from
        // which the result can be constructed.
        let blueprint_idx = self.chunk().push_constant(Value::Blueprint(lambda));

        let code_idx = self.push_op(
            if is_suspended_thunk {
                OpCode::OpThunkSuspended(blueprint_idx)
            } else {
                OpCode::OpThunkClosure(blueprint_idx)
            },
            node,
        );

        self.emit_upvalue_data(
            outer_slot,
            node,
            compiled.scope.upvalues,
            compiled.captures_with_stack,
        );

        if !is_suspended_thunk && !self.scope()[outer_slot].needs_finaliser {
            if !self.scope()[outer_slot].must_thunk {
                // The closure has upvalues, but is not recursive.  Therefore no thunk is required,
                // which saves us the overhead of Rc<RefCell<>>
                self.chunk()[code_idx] = OpCode::OpClosure(blueprint_idx);
            } else {
                // This case occurs when a closure has upvalue-references to itself but does not need a
                // finaliser.  Since no OpFinalise will be emitted later on we synthesize one here.
                // It is needed here only to set [`Closure::is_finalised`] which is used for sanity checks.
                #[cfg(debug_assertions)]
                self.push_op(
                    OpCode::OpFinalise(self.scope().stack_index(outer_slot)),
                    &self.span_for(node),
                );
            }
        }
    }

    fn compile_apply(&mut self, slot: LocalIdx, node: &ast::Apply) {
        // To call a function, we leave its arguments on the stack,
        // followed by the function expression itself, and then emit a
        // call instruction. This way, the stack is perfectly laid out
        // to enter the function call straight away.
        self.compile(slot, node.argument().unwrap());
        self.compile(slot, node.lambda().unwrap());
        self.emit_force(&node.lambda().unwrap());
        self.push_op(OpCode::OpCall, node);
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
        for upvalue in upvalues {
            match upvalue.kind {
                UpvalueKind::Local(idx) => {
                    let target = &self.scope()[idx];
                    let stack_idx = self.scope().stack_index(idx);

                    // If the target is not yet initialised, we need to defer
                    // the local access
                    if !target.initialised {
                        self.push_op(OpCode::DataDeferredLocal(stack_idx), &upvalue.span);
                        self.scope_mut().mark_needs_finaliser(slot);
                    } else {
                        // a self-reference
                        if slot == idx {
                            self.scope_mut().mark_must_thunk(slot);
                        }
                        self.push_op(OpCode::DataStackIdx(stack_idx), &upvalue.span);
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
        self.emit_constant(Value::String(ident.clone().into()), ident);
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
        self.contexts.push(self.context().inherit());
    }

    /// Declare a local variable known in the scope that is being
    /// compiled by pushing it to the locals. This is used to
    /// determine the stack offset of variables.
    fn declare_local<S: Into<String>, N: ToSpan>(&mut self, node: &N, name: S) -> LocalIdx {
        let name = name.into();
        let depth = self.scope().scope_depth();

        // Do this little dance to turn name:&'a str into the same
        // string with &'static lifetime, as required by WarningKind
        if let Some((global_ident, _)) = self.globals.get_key_value(name.as_str()) {
            self.emit_warning(node, WarningKind::ShadowedGlobal(global_ident));
        }

        let span = self.span_for(node);
        let (idx, shadowed) = self.scope_mut().declare_local(name, span);

        if let Some(shadow_idx) = shadowed {
            let other = &self.scope()[shadow_idx];
            if other.depth == depth {
                self.emit_error(node, ErrorKind::VariableAlreadyDefined(other.span));
            }
        }

        idx
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
        self.errors.push(Error::new(kind, span))
    }
}

/// Convert a non-dynamic string expression to a string if possible.
fn expr_static_str(node: &ast::Str) -> Option<SmolStr> {
    let mut parts = node.normalized_parts();

    if parts.len() != 1 {
        return None;
    }

    if let Some(ast::InterpolPart::Literal(lit)) = parts.pop() {
        return Some(SmolStr::new(lit));
    }

    None
}

/// Convert the provided `ast::Attr` into a statically known string if
/// possible.
fn expr_static_attr_str(node: &ast::Attr) -> Option<SmolStr> {
    match node {
        ast::Attr::Ident(ident) => Some(ident.ident_token().unwrap().text().into()),
        ast::Attr::Str(s) => expr_static_str(s),

        // The dynamic node type is just a wrapper. C++ Nix does not care
        // about the dynamic wrapper when determining whether the node
        // itself is dynamic, it depends solely on the expression inside
        // (i.e. `let ${"a"} = 1; in a` is valid).
        ast::Attr::Dynamic(ref dynamic) => match dynamic.expr().unwrap() {
            ast::Expr::Str(s) => expr_static_str(&s),
            _ => None,
        },
    }
}

/// Create a delayed source-only builtin compilation, for a builtin
/// which is written in Nix code.
///
/// **Important:** tvix *panics* if a builtin with invalid source code
/// is supplied. This is because there is no user-friendly way to
/// thread the errors out of this function right now.
fn compile_src_builtin(
    name: &'static str,
    code: &str,
    source: &SourceCode,
    weak: &Weak<GlobalsMap>,
) -> Value {
    use std::fmt::Write;

    let parsed = rnix::ast::Root::parse(code);

    if !parsed.errors().is_empty() {
        let mut out = format!("BUG: code for source-builtin '{}' had parser errors", name);
        for error in parsed.errors() {
            writeln!(out, "{}", error).unwrap();
        }

        panic!("{}", out);
    }

    let file = source.add_file(format!("<src-builtins/{}.nix>", name), code.to_string());
    let weak = weak.clone();

    Value::Thunk(Thunk::new_suspended_native(Box::new(move || {
        let result = compile(
            &parsed.tree().expr().unwrap(),
            None,
            file.clone(),
            weak.upgrade().unwrap(),
            &mut crate::observer::NoOpObserver {},
        )
        .map_err(|e| ErrorKind::NativeError {
            gen_type: "derivation",
            err: Box::new(e),
        })?;

        if !result.errors.is_empty() {
            return Err(ErrorKind::ImportCompilerError {
                path: format!("src-builtins/{}.nix", name).into(),
                errors: result.errors,
            });
        }

        Ok(Value::Thunk(Thunk::new_suspended(
            result.lambda,
            LightSpan::Actual { span: file.span },
        )))
    })))
}

/// Prepare the full set of globals available in evaluated code. These
/// are constructed from the set of builtins supplied by the caller,
/// which are made available globally under the `builtins` identifier.
///
/// A subset of builtins (specified by [`GLOBAL_BUILTINS`]) is
/// available globally *iff* they are set.
///
/// Optionally adds the `import` feature if desired by the caller.
pub fn prepare_globals(
    builtins: Vec<(&'static str, Value)>,
    src_builtins: Vec<(&'static str, &'static str)>,
    source: SourceCode,
    enable_import: bool,
) -> Rc<GlobalsMap> {
    Rc::new_cyclic(Box::new(move |weak: &Weak<GlobalsMap>| {
        // First step is to construct the builtins themselves as
        // `NixAttrs`.
        let mut builtins: GlobalsMap = HashMap::from_iter(builtins.into_iter());

        // At this point, optionally insert `import` if enabled. To
        // "tie the knot" of `import` needing the full set of globals
        // to instantiate its compiler, the `Weak` reference is passed
        // here.
        if enable_import {
            let import = Value::Builtin(import::builtins_import(weak, source.clone()));
            builtins.insert("import", import);
        }

        // Next, the actual map of globals which the compiler will use
        // to resolve identifiers is constructed.
        let mut globals: GlobalsMap = HashMap::new();

        // builtins contain themselves (`builtins.builtins`), which we
        // can resolve by manually constructing a suspended thunk that
        // dereferences the same weak pointer as above.
        let weak_globals = weak.clone();
        builtins.insert(
            "builtins",
            Value::Thunk(Thunk::new_suspended_native(Box::new(move || {
                Ok(weak_globals
                    .upgrade()
                    .unwrap()
                    .get("builtins")
                    .cloned()
                    .unwrap())
            }))),
        );

        // Insert top-level static value builtins.
        globals.insert("true", Value::Bool(true));
        globals.insert("false", Value::Bool(false));
        globals.insert("null", Value::Null);

        // If "source builtins" were supplied, compile them and insert
        // them.
        builtins.extend(src_builtins.into_iter().map(move |(name, code)| {
            let compiled = compile_src_builtin(name, code, &source, weak);
            (name, compiled)
        }));

        // Construct the actual `builtins` attribute set and insert it
        // in the global scope.
        globals.insert(
            "builtins",
            Value::attrs(NixAttrs::from_iter(builtins.clone().into_iter())),
        );

        // Finally, the builtins that should be globally available are
        // "elevated" to the outer scope.
        for global in GLOBAL_BUILTINS {
            if let Some(builtin) = builtins.get(global).cloned() {
                globals.insert(global, builtin);
            }
        }

        globals
    }))
}

pub fn compile(
    expr: &ast::Expr,
    location: Option<PathBuf>,
    file: Arc<codemap::File>,
    globals: Rc<GlobalsMap>,
    observer: &mut dyn CompilerObserver,
) -> EvalResult<CompilationOutput> {
    let mut c = Compiler::new(location, file, globals.clone(), observer)?;

    let root_span = c.span_for(expr);
    let root_slot = c.scope_mut().declare_phantom(root_span, false);
    c.compile(root_slot, expr.clone());

    // The final operation of any top-level Nix program must always be
    // `OpForce`. A thunk should not be returned to the user in an
    // unevaluated state (though in practice, a value *containing* a
    // thunk might be returned).
    c.emit_force(expr);
    c.push_op(OpCode::OpReturn, &root_span);

    let lambda = Rc::new(c.contexts.pop().unwrap().lambda);
    c.observer.observe_compiled_toplevel(&lambda);

    Ok(CompilationOutput {
        lambda,
        warnings: c.warnings,
        errors: c.errors,
        globals,
    })
}
