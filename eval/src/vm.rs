//! This module implements the virtual (or abstract) machine that runs
//! Tvix bytecode.

use std::rc::Rc;

use crate::{
    chunk::Chunk,
    errors::{ErrorKind, EvalResult},
    opcode::OpCode,
    value::{Lambda, NixAttrs, NixList, Value},
};

#[cfg(feature = "disassembler")]
use crate::disassembler::Tracer;

pub struct VM {
    ip: usize,
    chunk: Chunk,
    stack: Vec<Value>,

    // Stack indices of attribute sets from which variables should be
    // dynamically resolved (`with`).
    with_stack: Vec<usize>,
}

macro_rules! arithmetic_op {
    ( $self:ident, $op:tt ) => {{
        let b = $self.pop();
        let a = $self.pop();
        let result = arithmetic_op!(a, b, $op);
        $self.push(result);
    }};

    ( $a:ident, $b:ident, $op:tt ) => {{
        match ($a, $b) {
            (Value::Integer(i1), Value::Integer(i2)) => Value::Integer(i1 $op i2),
            (Value::Float(f1), Value::Float(f2)) => Value::Float(f1 $op f2),
            (Value::Integer(i1), Value::Float(f2)) => Value::Float(i1 as f64 $op f2),
            (Value::Float(f1), Value::Integer(i2)) => Value::Float(f1 $op i2 as f64),

            (v1, v2) => return Err(ErrorKind::TypeError {
                expected: "number (either int or float)",
                actual: if v1.is_number() {
                    v2.type_of()
                } else {
                    v1.type_of()
                },
            }.into()),
        }
    }};
}

macro_rules! cmp_op {
    ( $self:ident, $op:tt ) => {{
        let b = $self.pop();
        let a = $self.pop();

        // Comparable (in terms of ordering) values are numbers and
        // strings. Numbers need to be coerced similarly to arithmetic
        // ops if mixed types are encountered.
        let result = match (a, b) {
            (Value::Integer(i1), Value::Integer(i2)) => i1 $op i2,
            (Value::Float(f1), Value::Float(f2)) => f1 $op f2,
            (Value::Integer(i1), Value::Float(f2)) => (i1 as f64) $op f2,
            (Value::Float(f1), Value::Integer(i2)) => f1 $op (i2 as f64),
            (Value::String(s1), Value::String(s2)) => s1 $op s2,

            (lhs, rhs) => return Err(ErrorKind::Incomparable {
                lhs: lhs.type_of(),
                rhs: rhs.type_of(),
            }.into()),
        };

        $self.push(Value::Bool(result));
    }};
}

impl VM {
    fn inc_ip(&mut self) -> OpCode {
        let op = self.chunk.code[self.ip];
        self.ip += 1;
        op
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("runtime stack empty")
    }

    fn push(&mut self, value: Value) {
        self.stack.push(value)
    }

    fn peek(&self, offset: usize) -> &Value {
        &self.stack[self.stack.len() - 1 - offset]
    }

    fn run(&mut self) -> EvalResult<Value> {
        #[cfg(feature = "disassembler")]
        let mut tracer = Tracer::new();

        'dispatch: loop {
            let op = self.inc_ip();
            match op {
                OpCode::OpConstant(idx) => {
                    let c = self.chunk.constant(idx).clone();
                    self.push(c);
                }

                OpCode::OpPop => {
                    self.pop();
                }

                OpCode::OpAdd => {
                    let b = self.pop();
                    let a = self.pop();

                    let result = if let (Value::String(s1), Value::String(s2)) = (&a, &b) {
                        Value::String(s1.concat(s2))
                    } else {
                        arithmetic_op!(a, b, +)
                    };

                    self.push(result)
                }

                OpCode::OpSub => arithmetic_op!(self, -),
                OpCode::OpMul => arithmetic_op!(self, *),
                OpCode::OpDiv => arithmetic_op!(self, /),

                OpCode::OpInvert => {
                    let v = self.pop().as_bool()?;
                    self.push(Value::Bool(!v));
                }

                OpCode::OpNegate => match self.pop() {
                    Value::Integer(i) => self.push(Value::Integer(-i)),
                    Value::Float(f) => self.push(Value::Float(-f)),
                    v => {
                        return Err(ErrorKind::TypeError {
                            expected: "number (either int or float)",
                            actual: v.type_of(),
                        }
                        .into())
                    }
                },

                OpCode::OpEqual => {
                    let v2 = self.pop();
                    let v1 = self.pop();

                    self.push(Value::Bool(v1 == v2))
                }

                OpCode::OpLess => cmp_op!(self, <),
                OpCode::OpLessOrEq => cmp_op!(self, <=),
                OpCode::OpMore => cmp_op!(self, >),
                OpCode::OpMoreOrEq => cmp_op!(self, >=),

                OpCode::OpNull => self.push(Value::Null),
                OpCode::OpTrue => self.push(Value::Bool(true)),
                OpCode::OpFalse => self.push(Value::Bool(false)),

                OpCode::OpAttrs(count) => self.run_attrset(count)?,
                OpCode::OpAttrPath(count) => self.run_attr_path(count)?,

                OpCode::OpAttrsUpdate => {
                    let rhs = self.pop().to_attrs()?;
                    let lhs = self.pop().to_attrs()?;

                    self.push(Value::Attrs(Rc::new(lhs.update(&rhs))))
                }

                OpCode::OpAttrsSelect => {
                    let key = self.pop().to_string()?;
                    let attrs = self.pop().to_attrs()?;

                    match attrs.select(key.as_str()) {
                        Some(value) => self.push(value.clone()),

                        None => {
                            return Err(ErrorKind::AttributeNotFound {
                                name: key.as_str().to_string(),
                            }
                            .into())
                        }
                    }
                }

                OpCode::OpAttrOrNotFound => {
                    let key = self.pop().to_string()?;
                    let value = match self.pop() {
                        Value::Attrs(attrs) => match attrs.select(key.as_str()) {
                            Some(value) => value.clone(),
                            None => Value::NotFound,
                        },

                        _ => Value::NotFound,
                    };

                    self.push(value);
                }

                OpCode::OpAttrsIsSet => {
                    let key = self.pop().to_string()?;
                    let result = match self.pop() {
                        Value::Attrs(attrs) => attrs.select(key.as_str()).is_some(),

                        // Nix allows use of `?` on non-set types, but
                        // always returns false in those cases.
                        _ => false,
                    };

                    self.push(Value::Bool(result));
                }

                OpCode::OpList(count) => {
                    let list =
                        NixList::construct(count, self.stack.split_off(self.stack.len() - count));
                    self.push(Value::List(list));
                }

                OpCode::OpConcat => {
                    let rhs = self.pop().to_list()?;
                    let lhs = self.pop().to_list()?;
                    self.push(Value::List(lhs.concat(&rhs)))
                }

                OpCode::OpInterpolate(count) => self.run_interpolate(count)?,

                OpCode::OpJump(offset) => {
                    self.ip += offset;
                }

                OpCode::OpJumpIfTrue(offset) => {
                    if self.peek(0).as_bool()? {
                        self.ip += offset;
                    }
                }

                OpCode::OpJumpIfFalse(offset) => {
                    if !self.peek(0).as_bool()? {
                        self.ip += offset;
                    }
                }

                OpCode::OpJumpIfNotFound(offset) => {
                    if matches!(self.peek(0), Value::NotFound) {
                        self.pop();
                        self.ip += offset;
                    }
                }

                // These assertion operations error out if the stack
                // top is not of the expected type. This is necessary
                // to implement some specific behaviours of Nix
                // exactly.
                OpCode::OpAssertBool => {
                    let val = self.peek(0);
                    if !val.is_bool() {
                        return Err(ErrorKind::TypeError {
                            expected: "bool",
                            actual: val.type_of(),
                        }
                        .into());
                    }
                }

                // Remove the given number of elements from the stack,
                // but retain the top value.
                OpCode::OpCloseScope(count) => {
                    // Immediately move the top value into the right
                    // position.
                    let target_idx = self.stack.len() - 1 - count;
                    self.stack[target_idx] = self.pop();

                    // Then drop the remaining values.
                    for _ in 0..(count - 1) {
                        self.pop();
                    }
                }

                OpCode::OpGetLocal(local_idx) => {
                    let value = self.stack[local_idx].clone();
                    self.push(value)
                }

                OpCode::OpPushWith(idx) => self.with_stack.push(idx),
                OpCode::OpPopWith => {
                    self.with_stack.pop();
                }

                OpCode::OpResolveWith => {
                    let ident = self.pop().to_string()?;

                    // Attempt to resolve the variable, starting at
                    // the back of the with_stack.
                    'with: for idx in self.with_stack.iter().rev() {
                        let with = self.stack[*idx].as_attrs()?;
                        match with.select(ident.as_str()) {
                            None => continue 'with,
                            Some(val) => {
                                self.push(val.clone());
                                continue 'dispatch;
                            }
                        }
                    }

                    return Err(ErrorKind::UnknownDynamicVariable(ident.to_string()).into());
                }

                OpCode::OpAssert => {
                    if !self.pop().as_bool()? {
                        return Err(ErrorKind::AssertionFailed.into());
                    }
                }
            }

            #[cfg(feature = "disassembler")]
            {
                tracer.trace(&op, self.ip, &self.stack);
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
            path.push(self.pop().to_string()?);
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
            out.push_str(self.pop().to_string()?.as_str());
        }

        self.push(Value::String(out.into()));
        Ok(())
    }
}

pub fn run_lambda(lambda: Lambda) -> EvalResult<Value> {
    let mut vm = VM {
        chunk: Rc::<Chunk>::try_unwrap(lambda.chunk).unwrap(),
        ip: 0,
        stack: vec![],
        with_stack: vec![],
    };

    vm.run()
}
