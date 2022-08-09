//! This module implements the virtual (or abstract) machine that runs
//! Tvix bytecode.

use std::{collections::BTreeMap, rc::Rc};

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
    fn push(&mut self, value: Value) {
        self.stack.push(value)
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("TODO")
    }

    fn peek(&self, at: usize) -> &Value {
        &self.stack[self.stack.len() - 1 - at]
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

    fn run_attrset(&mut self, count: usize) -> EvalResult<()> {
        // If the attribute count happens to be 2, we might be able to
        // create the optimised name/value struct instead.
        if count == 2 {
            // When determining whether we are dealing with a
            // name/value pair, we return the stack locations of name
            // and value, using `0` as a sentinel value (i.e. if
            // either is 0, we are dealing with some other attrset).
            let is_pair = {
                // The keys are located 1 & 3 values back in the
                // stack.
                let k1 = self.peek(1);
                let k2 = self.peek(3);

                match (k1, k2) {
                    (Value::String(NixString(s1)), Value::String(NixString(s2)))
                        if (s1 == "name" && s2 == "value") =>
                    {
                        (1, 2)
                    }

                    (Value::String(NixString(s1)), Value::String(NixString(s2)))
                        if (s1 == "value" && s2 == "name") =>
                    {
                        (2, 1)
                    }

                    // Technically this branch lets type errors pass,
                    // but they will be caught during normal attribute
                    // set construction instead.
                    _ => (0, 0),
                }
            };

            match is_pair {
                (1, 2) => {
                    // The value of 'name' is at stack slot 0, the
                    // value of 'value' is at stack slot 2.
                    let pair = Value::Attrs(Rc::new(NixAttrs::KV {
                        name: self.pop(),
                        value: {
                            self.pop(); // ignore the key
                            self.pop()
                        },
                    }));

                    // Clean up the last key fragment.
                    self.pop();

                    self.push(pair);
                    return Ok(());
                }

                (2, 1) => {
                    // The value of 'name' is at stack slot 2, the
                    // value of 'value' is at stack slot 0.
                    let pair = Value::Attrs(Rc::new(NixAttrs::KV {
                        value: self.pop(),
                        name: {
                            self.pop(); // ignore the key
                            self.pop()
                        },
                    }));

                    // Clean up the last key fragment.
                    self.pop();

                    self.push(pair);
                    return Ok(());
                }
                _ => {}
            }
        }

        let mut attrs: BTreeMap<NixString, Value> = BTreeMap::new();

        for _ in 0..count {
            let value = self.pop();
            let key = self.pop().as_string()?; // TODO(tazjin): attrpath

            if attrs.insert(key.clone(), value).is_some() {
                return Err(Error::DuplicateAttrsKey { key: key.0 });
            }
        }

        // TODO(tazjin): extend_reserve(count) (rust#72631)

        self.push(Value::Attrs(Rc::new(NixAttrs::Map(attrs))));
        Ok(())
    }

    // Construct runtime representation of an attr path (essentially
    // just a list of strings).
    //
    // The difference to the list construction operation is that this
    // forces all elements into strings, as attribute set keys are
    // required to be strict in Nix.
    fn run_attr_path(&mut self, count: usize) -> EvalResult<()> {
        let mut path = vec![NixString(String::new()); count];

        for idx in 0..count {
            path[count - idx - 1] = self.pop().as_string()?
        }

        self.push(Value::AttrPath(path));
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
