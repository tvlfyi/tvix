//! This module implements the virtual (or abstract) machine that runs
//! Tvix bytecode.

use crate::{
    chunk::Chunk,
    errors::{Error, EvalResult},
    opcode::OpCode,
    value::{NumberPair, Value},
};

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

    fn pop_number_pair(&mut self) -> EvalResult<NumberPair> {
        let v2 = self.pop();
        let v1 = self.pop();

        match (v1, v2) {
            (Value::Integer(i1), Value::Integer(i2)) => Ok(NumberPair::Integer(i1, i2)),

            (Value::Float(f1), Value::Float(f2)) => Ok(NumberPair::Floats(f1, f2)),

            (Value::Integer(i1), Value::Float(f2)) => Ok(NumberPair::Floats(i1 as f64, f2)),

            (Value::Float(f1), Value::Integer(i2)) => Ok(NumberPair::Floats(f1, i2 as f64)),

            _ => Err(Error::TypeError {
                expected: "number (either int or float)",
                actual: if v1.is_number() {
                    v2.type_of()
                } else {
                    v1.type_of()
                },
            }),
        }
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

                OpCode::OpAdd => match self.pop_number_pair()? {
                    NumberPair::Floats(f1, f2) => self.push(Value::Float(f1 + f2)),
                    NumberPair::Integer(i1, i2) => self.push(Value::Integer(i1 + i2)),
                },

                OpCode::OpSub => match self.pop_number_pair()? {
                    NumberPair::Floats(f1, f2) => self.push(Value::Float(f1 - f2)),
                    NumberPair::Integer(i1, i2) => self.push(Value::Integer(i1 - i2)),
                },

                OpCode::OpMul => match self.pop_number_pair()? {
                    NumberPair::Floats(f1, f2) => self.push(Value::Float(f1 * f2)),
                    NumberPair::Integer(i1, i2) => self.push(Value::Integer(i1 * i2)),
                },

                OpCode::OpDiv => match self.pop_number_pair()? {
                    NumberPair::Floats(f1, f2) => self.push(Value::Float(f1 / f2)),
                    NumberPair::Integer(i1, i2) => self.push(Value::Integer(i1 / i2)),
                },

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
