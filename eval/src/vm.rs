//! This module implements the virtual (or abstract) machine that runs
//! Tvix bytecode.

use crate::{chunk::Chunk, errors::EvalResult, opcode::OpCode, value::Value};

pub struct VM {
    ip: usize,
    chunk: Chunk,
    stack: Vec<Value>,
}

impl VM {
    fn push(&mut self, value: Value) {
        self.stack.push(value)
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("TODO")
    }

    fn inc_ip(&mut self) -> OpCode {
        let op = self.chunk.code[self.ip];
        self.ip += 1;
        op
    }

    fn run(&mut self) -> EvalResult<Value> {
        loop {
            match self.inc_ip() {
                OpCode::OpConstant(idx) => {
                    let c = self.chunk.constant(idx).clone();
                    self.push(c);
                }

                OpCode::OpNull => todo!(),
                OpCode::OpTrue => todo!(),
                OpCode::OpFalse => todo!(),
            }

            if self.ip == self.chunk.code.len() {
                return Ok(self.pop());
            }
        }
    }
}

pub fn run_chunk(chunk: Chunk) -> EvalResult<Value> {
    let mut vm = VM {
        chunk,
        ip: 0,
        stack: vec![],
    };

    vm.run()
}
