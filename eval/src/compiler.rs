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

use crate::chunk::Chunk;
use crate::errors::EvalResult;
use crate::opcode::{CodeIdx, OpCode};
use crate::value::Value;

use rnix;
use rnix::types::{BinOpKind, EntryHolder, TokenWrapper, TypedNode, Wrapper};

struct Compiler {
    chunk: Chunk,
}

impl Compiler {
    fn compile(&mut self, node: rnix::SyntaxNode) -> EvalResult<()> {
        match node.kind() {
            // Root of a file contains no content, it's just a marker
            // type.
            rnix::SyntaxKind::NODE_ROOT => self.compile(node.first_child().expect("TODO")),

            // Literals contain a single token comprising of the
            // literal itself.
            rnix::SyntaxKind::NODE_LITERAL => {
                let value = rnix::types::Value::cast(node).unwrap();
                self.compile_literal(value.to_value().expect("TODO"))
            }

            rnix::SyntaxKind::NODE_STRING => {
                let op = rnix::types::Str::cast(node).unwrap();
                self.compile_string(op)
            }

            // The interpolation node is just a wrapper around the
            // inner value of a fragment, it only requires unwrapping.
            rnix::SyntaxKind::NODE_STRING_INTERPOL => {
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

            rnix::SyntaxKind::NODE_LIST => {
                let node = rnix::types::List::cast(node).unwrap();
                self.compile_list(node)
            }

            rnix::SyntaxKind::NODE_IF_ELSE => {
                let node = rnix::types::IfElse::cast(node).unwrap();
                self.compile_if_else(node)
            }

            kind => {
                println!("visiting unsupported node: {:?}", kind);
                Ok(())
            }
        }
    }

    fn compile_literal(&mut self, value: rnix::value::Value) -> EvalResult<()> {
        match value {
            rnix::NixValue::Float(f) => {
                let idx = self.chunk.add_constant(Value::Float(f));
                self.chunk.add_op(OpCode::OpConstant(idx));
                Ok(())
            }

            rnix::NixValue::Integer(i) => {
                let idx = self.chunk.add_constant(Value::Integer(i));
                self.chunk.add_op(OpCode::OpConstant(idx));
                Ok(())
            }

            rnix::NixValue::String(_) => todo!(),
            rnix::NixValue::Path(_, _) => todo!(),
        }
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
                    let idx = self.chunk.add_constant(Value::String(lit.into()));
                    self.chunk.add_op(OpCode::OpConstant(idx));
                }
            }
        }

        if count != 1 {
            self.chunk.add_op(OpCode::OpInterpolate(count));
        }

        Ok(())
    }

    fn compile_binop(&mut self, op: rnix::types::BinOp) -> EvalResult<()> {
        // Short-circuiting logical operators, which are under the
        // same node type as NODE_BIN_OP, but need to be handled
        // separately (i.e. before compiling the expressions used for
        // standard binary operators).
        match op.operator().unwrap() {
            BinOpKind::And => return self.compile_and(op),
            BinOpKind::Implication => todo!(),
            BinOpKind::Or => todo!(),

            _ => {}
        };

        self.compile(op.lhs().unwrap())?;
        self.compile(op.rhs().unwrap())?;

        match op.operator().unwrap() {
            BinOpKind::Add => self.chunk.add_op(OpCode::OpAdd),
            BinOpKind::Sub => self.chunk.add_op(OpCode::OpSub),
            BinOpKind::Mul => self.chunk.add_op(OpCode::OpMul),
            BinOpKind::Div => self.chunk.add_op(OpCode::OpDiv),
            BinOpKind::Update => self.chunk.add_op(OpCode::OpAttrsUpdate),
            BinOpKind::Equal => self.chunk.add_op(OpCode::OpEqual),
            BinOpKind::Less => self.chunk.add_op(OpCode::OpLess),
            BinOpKind::LessOrEq => self.chunk.add_op(OpCode::OpLessOrEq),
            BinOpKind::More => self.chunk.add_op(OpCode::OpMore),
            BinOpKind::MoreOrEq => self.chunk.add_op(OpCode::OpMoreOrEq),
            BinOpKind::Concat => self.chunk.add_op(OpCode::OpConcat),

            BinOpKind::NotEqual => {
                self.chunk.add_op(OpCode::OpEqual);
                self.chunk.add_op(OpCode::OpInvert)
            }

            BinOpKind::IsSet => todo!("? operator"),

            // Handled by separate branch above.
            BinOpKind::And | BinOpKind::Implication | BinOpKind::Or => unreachable!(),
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

        self.chunk.add_op(opcode);
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
            "true" => self.chunk.add_op(OpCode::OpTrue),
            "false" => self.chunk.add_op(OpCode::OpFalse),
            "null" => self.chunk.add_op(OpCode::OpNull),

            _ => todo!("identifier access"),
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
                            .add_constant(Value::String(ident.as_str().to_string().into()));
                        self.chunk.add_op(OpCode::OpConstant(idx));
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
                self.chunk.add_op(OpCode::OpAttrPath(2));
            }

            // The value is just compiled as normal so that its
            // resulting value is on the stack when the attribute set
            // is constructed at runtime.
            self.compile(kv.value().unwrap())?;
        }

        self.chunk.add_op(OpCode::OpAttrs(count));
        Ok(())
    }

    // Compile list literals into equivalent bytecode. List
    // construction is fairly simple, composing of pushing code for
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

        self.chunk.add_op(OpCode::OpList(count));
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

        let then_idx = self.chunk.add_op(OpCode::OpJumpIfFalse(0));

        self.chunk.add_op(OpCode::OpPop); // discard condition value
        self.compile(node.body().unwrap())?;

        let else_idx = self.chunk.add_op(OpCode::OpJump(0));

        self.patch_jump(then_idx); // patch jump *to* else_body
        self.chunk.add_op(OpCode::OpPop); // discard condition value
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
        let end_idx = self.chunk.add_op(OpCode::OpJumpIfFalse(0));

        // Otherwise, remove the previous value and leave the
        // right-hand side on the stack. Its result is now the value
        // of the whole expression.
        self.chunk.add_op(OpCode::OpPop);
        self.compile(node.rhs().unwrap())?;

        self.patch_jump(end_idx);

        Ok(())
    }

    fn patch_jump(&mut self, idx: CodeIdx) {
        let offset = self.chunk.code.len() - 1 - idx.0;

        match &mut self.chunk.code[idx.0] {
            OpCode::OpJump(n) => {
                *n = offset;
            }

            OpCode::OpJumpIfFalse(n) => {
                *n = offset;
            }

            op => panic!("attempted to patch unsupported op: {:?}", op),
        }
    }
}

pub fn compile(ast: rnix::AST) -> EvalResult<Chunk> {
    let mut c = Compiler {
        chunk: Chunk::default(),
    };

    c.compile(ast.node())?;

    Ok(c.chunk)
}
