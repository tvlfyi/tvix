//! This module implements a compiler for compiling the rnix AST
//! representation to Tvix bytecode.

use crate::chunk::Chunk;
use crate::errors::EvalResult;
use crate::opcode::OpCode;
use crate::value::Value;
use rnix;
use rnix::types::{TypedNode, Wrapper};

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

    fn compile_binop(&mut self, op: rnix::types::BinOp) -> EvalResult<()> {
        self.compile(op.lhs().unwrap())?;
        self.compile(op.rhs().unwrap())?;

        use rnix::types::BinOpKind;

        let opcode = match op.operator().unwrap() {
            BinOpKind::Add => OpCode::OpAdd,
            BinOpKind::Sub => OpCode::OpSub,
            BinOpKind::Mul => OpCode::OpMul,
            BinOpKind::Div => OpCode::OpDiv,
            BinOpKind::Equal => OpCode::OpEqual,
            _ => todo!(),
        };

        self.chunk.add_op(opcode);
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
}

pub fn compile(ast: rnix::AST) -> EvalResult<Chunk> {
    let mut c = Compiler {
        chunk: Chunk::default(),
    };

    c.compile(ast.node())?;

    Ok(c.chunk)
}
