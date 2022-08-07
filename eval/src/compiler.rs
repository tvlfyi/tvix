//! This module implements a compiler for compiling the rnix AST
//! representation to Tvix bytecode.

use crate::chunk::Chunk;
use crate::errors::EvalResult;
use crate::opcode::OpCode;
use crate::value::Value;
use rnix;

struct Compiler {
    chunk: Chunk,
}

impl Compiler {
    fn compile(&mut self, node: &rnix::SyntaxNode) -> EvalResult<()> {
        match node.kind() {
            // Root of a file contains no content, it's just a marker
            // type.
            rnix::SyntaxKind::NODE_ROOT => self.compile(&node.first_child().expect("TODO")),

            // Literals contain a single token comprising of the
            // literal itself.
            rnix::SyntaxKind::NODE_LITERAL => {
                let token = node.first_token().expect("TODO");
                self.compile_literal(token)
            }

            kind => {
                println!("visiting unsupported node: {:?}", kind);
                Ok(())
            }
        }
    }

    fn compile_literal(&mut self, token: rnix::SyntaxToken) -> EvalResult<()> {
        let value = rnix::value::Value::from_token(token.kind(), token.text()).expect("TODO");

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
}

pub fn compile(ast: rnix::AST) -> EvalResult<Chunk> {
    let mut c = Compiler {
        chunk: Chunk::default(),
    };

    c.compile(&ast.node())?;

    Ok(c.chunk)
}
