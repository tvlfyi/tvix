//! This module implements the virtual (or abstract) machine that runs
//! Tvix bytecode.

use std::rc::Rc;

use crate::{
    chunk::Chunk,
    errors::{Error, EvalResult},
    opcode::OpCode,
    value::{NixAttrs, NixList, NixString, Value},
};

pub struct VM {
    ip: usize,
    chunk: Chunk,
    stack: Vec<Value>,
}

impl VM {
    fn inc_ip(&mut self) -> OpCode {
        let op = self.chunk.code[self.ip];
        self.ip += 1;
        op
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

            (v1, v2) => Err(Error::TypeError {
                expected: "number (either int or float)",
                actual: if v1.is_number() {
                    v2.type_of()
                } else {
                    v1.type_of()
                },
            }),
        }
    }

    fn push(&mut self, value: Value) {
        self.stack.push(value)
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

                OpCode::OpInvert => {
                    let v = self.pop().as_bool()?;
                    self.push(Value::Bool(!v));
                }

                OpCode::OpNegate => match self.pop() {
                    Value::Integer(i) => self.push(Value::Integer(-i)),
                    Value::Float(f) => self.push(Value::Float(-f)),
                    v => {
                        return Err(Error::TypeError {
                            expected: "number (either int or float)",
                            actual: v.type_of(),
                        })
                    }
                },

                OpCode::OpEqual => {
                    let v2 = self.pop();
                    let v1 = self.pop();

                    let eq = match (v1, v2) {
                        (Value::Float(f), Value::Integer(i))
                        | (Value::Integer(i), Value::Float(f)) => f == (i as f64),

                        (v1, v2) => v1 == v2,
                    };

                    self.push(Value::Bool(eq))
                }

                OpCode::OpNull => self.push(Value::Null),
                OpCode::OpTrue => self.push(Value::Bool(true)),
                OpCode::OpFalse => self.push(Value::Bool(false)),
                OpCode::OpAttrs(count) => self.run_attrset(count)?,
                OpCode::OpAttrPath(count) => self.run_attr_path(count)?,
                OpCode::OpList(count) => self.run_list(count)?,
                OpCode::OpInterpolate(count) => self.run_interpolate(count)?,
            }

            if self.ip == self.chunk.code.len() {
                return Ok(self.pop());
            }
        }
    }

    // Construct runtime representation of an attr path (essentially
    // just a list of strings).
    //
    // The difference to the list construction operation is that this
    // forces all elements into strings, as attribute set keys are
    // required to be strict in Nix.
    fn run_attr_path(&mut self, count: usize) -> EvalResult<()> {
        debug_assert!(count > 1, "AttrPath needs at least two fragments");
        let mut path = Vec::with_capacity(count);

        for _ in 0..count {
            path.push(self.pop().as_string()?);
        }

        self.push(Value::AttrPath(path));
        Ok(())
    }

    fn run_attrset(&mut self, count: usize) -> EvalResult<()> {
        let attrs = NixAttrs::construct(count, self.stack.split_off(self.stack.len() - count * 2))?;
        self.push(Value::Attrs(Rc::new(attrs)));
        Ok(())
    }

    // Interpolate string fragments by popping the specified number of
    // fragments of the stack, evaluating them to strings, and pushing
    // the concatenated result string back on the stack.
    fn run_interpolate(&mut self, count: usize) -> EvalResult<()> {
        let mut out = String::new();

        for _ in 0..count {
            out.push_str(&self.pop().as_string()?.0);
        }

        self.push(Value::String(NixString(out)));
        Ok(())
    }

    // Construct runtime representation of a list. Because the list
    // items are on the stack in reverse order, the vector is created
    // initialised and elements are directly assigned to their
    // respective indices.
    fn run_list(&mut self, count: usize) -> EvalResult<()> {
        let mut list = vec![Value::Null; count];

        for idx in 0..count {
            list[count - idx - 1] = self.pop();
        }

        self.push(Value::List(NixList(list)));
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumberPair {
    Floats(f64, f64),
    Integer(i64, i64),
}

pub fn run_chunk(chunk: Chunk) -> EvalResult<Value> {
    let mut vm = VM {
        chunk,
        ip: 0,
        stack: vec![],
    };

    vm.run()
}
