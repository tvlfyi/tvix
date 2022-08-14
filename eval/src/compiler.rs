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
//! variants are filed. In cases where the invariant is guaranteed by
//! the code in this module, `debug_assert!` has been used to catch
//! mistakes early during development.

use path_clean::PathClean;
use rnix::types::{BinOpKind, EntryHolder, TokenWrapper, TypedNode, Wrapper};
use std::path::{Path, PathBuf};

use crate::chunk::Chunk;
use crate::errors::{Error, EvalResult};
use crate::opcode::{CodeIdx, OpCode};
use crate::value::Value;
use crate::warnings::{EvalWarning, WarningKind};

/// Represents the result of compiling a piece of Nix code. If
/// compilation was successful, the resulting bytecode can be passed
/// to the VM.
pub struct CompilationResult {
    pub chunk: Chunk,
    pub warnings: Vec<EvalWarning>,
}

// Represents a single local already known to the compiler.
struct Local {
    // Definition name, which can be different kinds of tokens (plain
    // string or identifier). Nix does not allow dynamic names inside
    // of `let`-expressions.
    name: String,

    // Scope depth of this local.
    depth: usize,
}

/// Represents locals known during compilation, which can be resolved
/// directly to stack indices.
///
/// TODO(tazjin): `with`-stack
/// TODO(tazjin): flag "specials" (e.g. note depth if builtins are
/// overridden)
#[derive(Default)]
struct Locals {
    locals: Vec<Local>,

    // How many scopes "deep" are these locals?
    scope_depth: usize,
}

struct Compiler {
    chunk: Chunk,
    locals: Locals,

    warnings: Vec<EvalWarning>,
    root_dir: PathBuf,
}

impl Compiler {
    fn compile(&mut self, node: rnix::SyntaxNode) -> EvalResult<()> {
        match node.kind() {
            // Root of a file contains no content, it's just a marker
            // type.
            rnix::SyntaxKind::NODE_ROOT => self.compile(node.first_child().expect("TODO")),

            // Literals contain a single token consisting of the
            // literal itself.
            rnix::SyntaxKind::NODE_LITERAL => {
                let value = rnix::types::Value::cast(node).unwrap();
                self.compile_literal(value)
            }

            rnix::SyntaxKind::NODE_STRING => {
                let op = rnix::types::Str::cast(node).unwrap();
                self.compile_string(op)
            }

            // The interpolation & dynamic nodes are just wrappers
            // around the inner value of a fragment, they only require
            // unwrapping.
            rnix::SyntaxKind::NODE_STRING_INTERPOL | rnix::SyntaxKind::NODE_DYNAMIC => {
                self.compile(node.first_child().expect("TODO (should not be possible)"))
            }

            rnix::SyntaxKind::NODE_BIN_OP => {
                let op = rnix::types::BinOp::cast(node).expect("TODO (should not be possible)");
                self.compile_binop(op)
            }

            rnix::SyntaxKind::NODE_UNARY_OP => {
                let op = rnix::types::UnaryOp::cast(node).expect("TODO: (should not be possible)");
                self.compile_unary_op(op)
            }

            rnix::SyntaxKind::NODE_PAREN => {
                let node = rnix::types::Paren::cast(node).unwrap();
                self.compile(node.inner().unwrap())
            }

            rnix::SyntaxKind::NODE_IDENT => {
                let node = rnix::types::Ident::cast(node).unwrap();
                self.compile_ident(node)
            }

            rnix::SyntaxKind::NODE_ATTR_SET => {
                let node = rnix::types::AttrSet::cast(node).unwrap();
                self.compile_attr_set(node)
            }

            rnix::SyntaxKind::NODE_SELECT => {
                let node = rnix::types::Select::cast(node).unwrap();
                self.compile_select(node)
            }

            rnix::SyntaxKind::NODE_OR_DEFAULT => {
                let node = rnix::types::OrDefault::cast(node).unwrap();
                self.compile_or_default(node)
            }

            rnix::SyntaxKind::NODE_LIST => {
                let node = rnix::types::List::cast(node).unwrap();
                self.compile_list(node)
            }

            rnix::SyntaxKind::NODE_IF_ELSE => {
                let node = rnix::types::IfElse::cast(node).unwrap();
                self.compile_if_else(node)
            }

            rnix::SyntaxKind::NODE_LET_IN => {
                let node = rnix::types::LetIn::cast(node).unwrap();
                self.compile_let_in(node)
            }

            kind => panic!("visiting unsupported node: {:?}", kind),
        }
    }

    /// Compiles nodes the same way that `Self::compile` does, with
    /// the exception of identifiers which are added literally to the
    /// stack as string values.
    ///
    /// This is needed for correctly accessing attribute sets.
    fn compile_with_literal_ident(&mut self, node: rnix::SyntaxNode) -> EvalResult<()> {
        if node.kind() == rnix::SyntaxKind::NODE_IDENT {
            let ident = rnix::types::Ident::cast(node).unwrap();
            self.emit_literal_ident(&ident);
            return Ok(());
        }

        self.compile(node)
    }

    fn compile_literal(&mut self, node: rnix::types::Value) -> EvalResult<()> {
        match node.to_value().unwrap() {
            rnix::NixValue::Float(f) => {
                let idx = self.chunk.push_constant(Value::Float(f));
                self.chunk.push_op(OpCode::OpConstant(idx));
                Ok(())
            }

            rnix::NixValue::Integer(i) => {
                let idx = self.chunk.push_constant(Value::Integer(i));
                self.chunk.push_op(OpCode::OpConstant(idx));
                Ok(())
            }

            // These nodes are yielded by literal URL values.
            rnix::NixValue::String(s) => {
                self.warnings.push(EvalWarning {
                    node: node.node().clone(),
                    kind: WarningKind::DeprecatedLiteralURL,
                });

                let idx = self.chunk.push_constant(Value::String(s.into()));
                self.chunk.push_op(OpCode::OpConstant(idx));
                Ok(())
            }

            rnix::NixValue::Path(anchor, path) => self.compile_path(anchor, path),
        }
    }

    fn compile_path(&mut self, anchor: rnix::value::Anchor, path: String) -> EvalResult<()> {
        let path = match anchor {
            rnix::value::Anchor::Absolute => Path::new(&path).to_owned(),

            rnix::value::Anchor::Home => {
                let mut buf = dirs::home_dir().ok_or_else(|| {
                    Error::PathResolution("failed to determine home directory".into())
                })?;

                buf.push(&path);
                buf
            }

            rnix::value::Anchor::Relative => {
                let mut buf = self.root_dir.clone();
                buf.push(path);
                buf
            }

            // This confusingly named variant is actually
            // angle-bracket lookups, which in C++ Nix desugar
            // to calls to `__findFile` (implicitly in the
            // current scope).
            rnix::value::Anchor::Store => todo!("resolve <...> lookups at runtime"),
        };

        // TODO: Use https://github.com/rust-lang/rfcs/issues/2208
        // once it is available
        let value = Value::Path(path.clean());
        let idx = self.chunk.push_constant(value);
        self.chunk.push_op(OpCode::OpConstant(idx));

        Ok(())
    }

    fn compile_string(&mut self, string: rnix::types::Str) -> EvalResult<()> {
        let mut count = 0;

        // The string parts are produced in literal order, however
        // they need to be reversed on the stack in order to
        // efficiently create the real string in case of
        // interpolation.
        for part in string.parts().into_iter().rev() {
            count += 1;

            match part {
                // Interpolated expressions are compiled as normal and
                // dealt with by the VM before being assembled into
                // the final string.
                rnix::StrPart::Ast(node) => self.compile(node)?,

                rnix::StrPart::Literal(lit) => {
                    let idx = self.chunk.push_constant(Value::String(lit.into()));
                    self.chunk.push_op(OpCode::OpConstant(idx));
                }
            }
        }

        if count != 1 {
            self.chunk.push_op(OpCode::OpInterpolate(count));
        }

        Ok(())
    }

    fn compile_binop(&mut self, op: rnix::types::BinOp) -> EvalResult<()> {
        // Short-circuiting and other strange operators, which are
        // under the same node type as NODE_BIN_OP, but need to be
        // handled separately (i.e. before compiling the expressions
        // used for standard binary operators).
        match op.operator().unwrap() {
            BinOpKind::And => return self.compile_and(op),
            BinOpKind::Or => return self.compile_or(op),
            BinOpKind::Implication => return self.compile_implication(op),
            BinOpKind::IsSet => return self.compile_is_set(op),

            _ => {}
        };

        self.compile(op.lhs().unwrap())?;
        self.compile(op.rhs().unwrap())?;

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
            BinOpKind::And | BinOpKind::Implication | BinOpKind::Or | BinOpKind::IsSet => {
                unreachable!()
            }
        };

        Ok(())
    }

    fn compile_unary_op(&mut self, op: rnix::types::UnaryOp) -> EvalResult<()> {
        self.compile(op.value().unwrap())?;

        use rnix::types::UnaryOpKind;
        let opcode = match op.operator() {
            UnaryOpKind::Invert => OpCode::OpInvert,
            UnaryOpKind::Negate => OpCode::OpNegate,
        };

        self.chunk.push_op(opcode);
        Ok(())
    }

    fn compile_ident(&mut self, node: rnix::types::Ident) -> EvalResult<()> {
        match node.as_str() {
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
            "true" => self.chunk.push_op(OpCode::OpTrue),
            "false" => self.chunk.push_op(OpCode::OpFalse),
            "null" => self.chunk.push_op(OpCode::OpNull),

            name => {
                // Note: `with` and some other special scoping
                // features are not yet implemented.
                match self.resolve_local(name) {
                    Some(idx) => self.chunk.push_op(OpCode::OpGetLocal(idx)),
                    None => return Err(Error::UnknownStaticVariable(node)),
                }
            }
        };

        Ok(())
    }

    // Compile attribute set literals into equivalent bytecode.
    //
    // This is complicated by a number of features specific to Nix
    // attribute sets, most importantly:
    //
    // 1. Keys can be dynamically constructed through interpolation.
    // 2. Keys can refer to nested attribute sets.
    // 3. Attribute sets can (optionally) be recursive.
    fn compile_attr_set(&mut self, node: rnix::types::AttrSet) -> EvalResult<()> {
        if node.recursive() {
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
                        self.compile(from.inner().unwrap())?;
                        self.emit_literal_ident(&ident);
                        self.chunk.push_op(OpCode::OpAttrsSelect);
                    }
                }

                None => {
                    for ident in inherit.idents() {
                        count += 1;
                        self.emit_literal_ident(&ident);

                        match self.resolve_local(ident.as_str()) {
                            Some(idx) => self.chunk.push_op(OpCode::OpGetLocal(idx)),
                            None => return Err(Error::UnknownStaticVariable(ident)),
                        };
                    }
                }
            }
        }

        for kv in node.entries() {
            count += 1;

            // Because attribute set literals can contain nested keys,
            // there is potentially more than one key fragment. If
            // this is the case, a special operation to construct a
            // runtime value representing the attribute path is
            // emitted.
            let mut key_count = 0;
            for fragment in kv.key().unwrap().path() {
                key_count += 1;

                match fragment.kind() {
                    rnix::SyntaxKind::NODE_IDENT => {
                        let ident = rnix::types::Ident::cast(fragment).unwrap();

                        // TODO(tazjin): intern!
                        let idx = self
                            .chunk
                            .push_constant(Value::String(ident.as_str().into()));
                        self.chunk.push_op(OpCode::OpConstant(idx));
                    }

                    // For all other expression types, we simply
                    // compile them as normal. The operation should
                    // result in a string value, which is checked at
                    // runtime on construction.
                    _ => self.compile(fragment)?,
                }
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
            self.compile(kv.value().unwrap())?;
        }

        self.chunk.push_op(OpCode::OpAttrs(count));
        Ok(())
    }

    fn compile_select(&mut self, node: rnix::types::Select) -> EvalResult<()> {
        // Push the set onto the stack
        self.compile(node.set().unwrap())?;

        // Push the key and emit the access instruction.
        //
        // This order matters because the key needs to be evaluated
        // first to fail in the correct order on type errors.
        self.compile_with_literal_ident(node.index().unwrap())?;
        self.chunk.push_op(OpCode::OpAttrsSelect);

        Ok(())
    }

    // Compile list literals into equivalent bytecode. List
    // construction is fairly simple, consisting of pushing code for
    // each literal element and an instruction with the element count.
    //
    // The VM, after evaluating the code for each element, simply
    // constructs the list from the given number of elements.
    fn compile_list(&mut self, node: rnix::types::List) -> EvalResult<()> {
        let mut count = 0;

        for item in node.items() {
            count += 1;
            self.compile(item)?;
        }

        self.chunk.push_op(OpCode::OpList(count));
        Ok(())
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
    fn compile_if_else(&mut self, node: rnix::types::IfElse) -> EvalResult<()> {
        self.compile(node.condition().unwrap())?;

        let then_idx = self.chunk.push_op(OpCode::OpJumpIfFalse(0));

        self.chunk.push_op(OpCode::OpPop); // discard condition value
        self.compile(node.body().unwrap())?;

        let else_idx = self.chunk.push_op(OpCode::OpJump(0));

        self.patch_jump(then_idx); // patch jump *to* else_body
        self.chunk.push_op(OpCode::OpPop); // discard condition value
        self.compile(node.else_body().unwrap())?;

        self.patch_jump(else_idx); // patch jump *over* else body

        Ok(())
    }

    fn compile_and(&mut self, node: rnix::types::BinOp) -> EvalResult<()> {
        debug_assert!(
            matches!(node.operator(), Some(BinOpKind::And)),
            "compile_and called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack.
        self.compile(node.lhs().unwrap())?;

        // If this value is false, jump over the right-hand side - the
        // whole expression is false.
        let end_idx = self.chunk.push_op(OpCode::OpJumpIfFalse(0));

        // Otherwise, remove the previous value and leave the
        // right-hand side on the stack. Its result is now the value
        // of the whole expression.
        self.chunk.push_op(OpCode::OpPop);
        self.compile(node.rhs().unwrap())?;

        self.patch_jump(end_idx);
        self.chunk.push_op(OpCode::OpAssertBool);

        Ok(())
    }

    fn compile_or(&mut self, node: rnix::types::BinOp) -> EvalResult<()> {
        debug_assert!(
            matches!(node.operator(), Some(BinOpKind::Or)),
            "compile_or called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack
        self.compile(node.lhs().unwrap())?;

        // Opposite of above: If this value is **true**, we can
        // short-circuit the right-hand side.
        let end_idx = self.chunk.push_op(OpCode::OpJumpIfTrue(0));
        self.chunk.push_op(OpCode::OpPop);
        self.compile(node.rhs().unwrap())?;
        self.patch_jump(end_idx);
        self.chunk.push_op(OpCode::OpAssertBool);

        Ok(())
    }

    fn compile_implication(&mut self, node: rnix::types::BinOp) -> EvalResult<()> {
        debug_assert!(
            matches!(node.operator(), Some(BinOpKind::Implication)),
            "compile_implication called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Leave left-hand side value on the stack and invert it.
        self.compile(node.lhs().unwrap())?;
        self.chunk.push_op(OpCode::OpInvert);

        // Exactly as `||` (because `a -> b` = `!a || b`).
        let end_idx = self.chunk.push_op(OpCode::OpJumpIfTrue(0));
        self.chunk.push_op(OpCode::OpPop);
        self.compile(node.rhs().unwrap())?;
        self.patch_jump(end_idx);
        self.chunk.push_op(OpCode::OpAssertBool);

        Ok(())
    }

    fn compile_is_set(&mut self, node: rnix::types::BinOp) -> EvalResult<()> {
        debug_assert!(
            matches!(node.operator(), Some(BinOpKind::IsSet)),
            "compile_is_set called with wrong operator kind: {:?}",
            node.operator(),
        );

        // Put the attribute set on the stack.
        self.compile(node.lhs().unwrap())?;

        // If the key is a NODE_SELECT, the check is deeper than one
        // level and requires special handling.
        //
        // Otherwise, the right hand side is the (only) key expression
        // itself and can be compiled directly.
        let mut next = node.rhs().unwrap();
        let mut fragments = vec![];

        loop {
            if matches!(next.kind(), rnix::SyntaxKind::NODE_SELECT) {
                // Keep nesting deeper until we encounter something
                // different than `NODE_SELECT` on the left side. This is
                // required because `rnix` parses nested keys as select
                // expressions, instead of as a key expression.
                //
                // The parsed tree will nest something like `a.b.c.d.e.f`
                // as (((((a, b), c), d), e), f).
                fragments.push(next.last_child().unwrap());
                next = next.first_child().unwrap();
            } else {
                self.compile_with_literal_ident(next)?;

                for fragment in fragments.into_iter().rev() {
                    self.chunk.push_op(OpCode::OpAttrOrNotFound);
                    self.compile_with_literal_ident(fragment)?;
                }

                self.chunk.push_op(OpCode::OpAttrsIsSet);
                break;
            }
        }

        Ok(())
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
    fn compile_or_default(&mut self, node: rnix::types::OrDefault) -> EvalResult<()> {
        let select = node.index().unwrap();

        let mut next = select.set().unwrap();
        let mut fragments = vec![select.index().unwrap()];
        let mut jumps = vec![];

        loop {
            if matches!(next.kind(), rnix::SyntaxKind::NODE_SELECT) {
                fragments.push(next.last_child().unwrap());
                next = next.first_child().unwrap();
                continue;
            } else {
                self.compile(next)?;
            }

            for fragment in fragments.into_iter().rev() {
                self.compile_with_literal_ident(fragment)?;
                self.chunk.push_op(OpCode::OpAttrOrNotFound);
                jumps.push(self.chunk.push_op(OpCode::OpJumpIfNotFound(0)));
            }

            break;
        }

        let final_jump = self.chunk.push_op(OpCode::OpJump(0));
        for jump in jumps {
            self.patch_jump(jump);
        }

        // Compile the default value expression and patch the final
        // jump to point *beyond* it.
        self.compile(node.default().unwrap())?;
        self.patch_jump(final_jump);

        Ok(())
    }

    // Compile a standard `let ...; in ...` statement.
    //
    // Unless in a non-standard scope, the encountered values are
    // simply pushed on the stack and their indices noted in the
    // entries vector.
    fn compile_let_in(&mut self, node: rnix::types::LetIn) -> Result<(), Error> {
        self.begin_scope();
        let mut entries = vec![];
        let mut from_inherits = vec![];

        for inherit in node.inherits() {
            match inherit.from() {
                // Within a `let` binding, inheriting from the outer
                // scope is practically a no-op.
                None => {
                    self.warnings.push(EvalWarning {
                        node: inherit.node().clone(),
                        kind: WarningKind::UselessInherit,
                    });

                    continue;
                }

                Some(_) => {
                    for ident in inherit.idents() {
                        self.locals.locals.push(Local {
                            name: ident.as_str().to_string(),
                            depth: self.locals.scope_depth,
                        });
                    }
                    from_inherits.push(inherit);
                }
            }
        }

        // Before compiling the values of a let expression, all keys
        // need to already be added to the known locals. This is
        // because in Nix these bindings are always recursive (they
        // can even refer to themselves).
        for entry in node.entries() {
            let key = entry.key().unwrap();
            let mut path = normalise_ident_path(key.path())?;

            if path.len() != 1 {
                todo!("nested bindings in let expressions :(")
            }

            entries.push(entry.value().unwrap());

            self.locals.locals.push(Local {
                name: path.pop().unwrap(),
                depth: self.locals.scope_depth,
            });
        }

        // Now we can add instructions to look up each inherited value
        // ...
        for inherit in from_inherits {
            let from = inherit
                .from()
                .expect("only inherits with `from` are pushed here");

            for ident in inherit.idents() {
                // TODO: Optimised multi-select instruction?
                self.compile(from.inner().unwrap())?;
                self.emit_literal_ident(&ident);
                self.chunk.push_op(OpCode::OpAttrsSelect);
            }
        }

        // ... and finally each expression, leaving the values on the
        // stack in the right order.
        for value in entries {
            self.compile(value)?;
        }

        // Deal with the body, then clean up the locals afterwards.
        self.compile(node.body().unwrap())?;
        self.end_scope();
        Ok(())
    }

    // Emit the literal string value of an identifier. Required for
    // several operations related to attribute sets, where identifiers
    // are used as string keys.
    fn emit_literal_ident(&mut self, ident: &rnix::types::Ident) {
        let idx = self
            .chunk
            .push_constant(Value::String(ident.as_str().into()));
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
        self.locals.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        let mut scope = &mut self.locals;
        debug_assert!(scope.scope_depth != 0, "can not end top scope");
        scope.scope_depth -= 1;

        // When ending a scope, all corresponding locals need to be
        // removed, but the value of the body needs to remain on the
        // stack. This is implemented by a separate instruction.
        let mut pops = 0;

        // TL;DR - iterate from the back while things belonging to the
        // ended scope still exist.
        while !scope.locals.is_empty()
            && scope.locals[scope.locals.len() - 1].depth > scope.scope_depth
        {
            pops += 1;
            scope.locals.pop();
        }

        if pops > 0 {
            self.chunk.push_op(OpCode::OpCloseScope(pops));
        }
    }

    fn resolve_local(&mut self, name: &str) -> Option<usize> {
        let scope = &self.locals;

        for (idx, local) in scope.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(idx);
            }
        }

        None
    }
}

/// Convert a single identifier path fragment to a string if possible,
/// or raise an error about the node being dynamic.
fn ident_fragment_to_string(node: rnix::SyntaxNode) -> EvalResult<String> {
    match node.kind() {
        rnix::SyntaxKind::NODE_IDENT => {
            Ok(rnix::types::Ident::cast(node).unwrap().as_str().to_string())
        }

        rnix::SyntaxKind::NODE_STRING => {
            let s = rnix::types::Str::cast(node).unwrap();
            let mut parts = s.parts();

            if parts.len() == 1 {
                if let rnix::value::StrPart::Literal(lit) = parts.pop().unwrap() {
                    return Ok(lit);
                }
            }

            return Err(Error::DynamicKeyInLet(s.node().clone()));
        }

        // The dynamic node type is just a wrapper and we recurse to
        // its inner node. C++ Nix does not care about the dynamic
        // wrapper when determining whether the node itself is
        // dynamic, it depends solely on the expression inside (i.e.
        // `let ${"a"} = 1; in a` is valid)
        rnix::SyntaxKind::NODE_DYNAMIC => {
            ident_fragment_to_string(rnix::types::Dynamic::cast(node).unwrap().inner().unwrap())
        }

        _ => Err(Error::DynamicKeyInLet(node)),
    }
}

// Normalises identifier fragments into a single string vector for
// `let`-expressions; fails if fragments requiring dynamic computation
// are encountered.
fn normalise_ident_path<I: Iterator<Item = rnix::SyntaxNode>>(path: I) -> EvalResult<Vec<String>> {
    path.map(ident_fragment_to_string).collect()
}

pub fn compile(ast: rnix::AST, location: Option<PathBuf>) -> EvalResult<CompilationResult> {
    let mut root_dir = match location {
        Some(dir) => Ok(dir),
        None => std::env::current_dir().map_err(|e| {
            Error::PathResolution(format!("could not determine current directory: {}", e))
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
        locals: Default::default(),
    };

    c.compile(ast.node())?;

    Ok(CompilationResult {
        chunk: c.chunk,
        warnings: c.warnings,
    })
}
