//! This module implements the virtual (or abstract) machine that runs
//! Tvix bytecode.

use std::{cell::RefMut, rc::Rc};

use crate::{
    chunk::Chunk,
    errors::{Error, ErrorKind, EvalResult},
    opcode::{CodeIdx, ConstantIdx, Count, JumpOffset, OpCode, StackIdx, UpvalueIdx},
    upvalues::UpvalueCarrier,
    value::{Closure, Lambda, NixAttrs, NixList, Thunk, Value},
};

struct CallFrame {
    lambda: Rc<Lambda>,
    upvalues: Vec<Value>,
    ip: usize,
    stack_offset: usize,
}

impl CallFrame {
    /// Retrieve an upvalue from this frame at the given index.
    fn upvalue(&self, idx: UpvalueIdx) -> &Value {
        &self.upvalues[idx.0]
    }
}

pub struct VM {
    frames: Vec<CallFrame>,
    stack: Vec<Value>,

    // Stack indices of attribute sets from which variables should be
    // dynamically resolved (`with`).
    with_stack: Vec<usize>,

    #[cfg(feature = "disassembler")]
    pub tracer: crate::disassembler::Tracer,
}

/// This macro wraps a computation that returns an ErrorKind or a
/// result, and wraps the ErrorKind in an Error struct if present.
///
/// The reason for this macro's existence is that calculating spans is
/// potentially expensive, so it should be avoided to the last moment
/// (i.e. definite instantiation of a runtime error) if possible.
macro_rules! fallible {
    ( $self:ident, $body:expr) => {
        match $body {
            Ok(result) => result,
            Err(kind) => {
                return Err(Error {
                    kind,
                    span: $self.current_span(),
                })
            }
        }
    };
}

#[macro_export]
macro_rules! arithmetic_op {
    ( $self:ident, $op:tt ) => {{
        let b = $self.pop();
        let a = $self.pop();
        let result = fallible!($self, arithmetic_op!(a, b, $op));
        $self.push(result);
    }};

    ( $a:ident, $b:ident, $op:tt ) => {{
        match ($a, $b) {
            (Value::Integer(i1), Value::Integer(i2)) => Ok(Value::Integer(i1 $op i2)),
            (Value::Float(f1), Value::Float(f2)) => Ok(Value::Float(f1 $op f2)),
            (Value::Integer(i1), Value::Float(f2)) => Ok(Value::Float(i1 as f64 $op f2)),
            (Value::Float(f1), Value::Integer(i2)) => Ok(Value::Float(f1 $op i2 as f64)),

            (v1, v2) => Err(ErrorKind::TypeError {
                expected: "number (either int or float)",
                actual: if v1.is_number() {
                    v2.type_of()
                } else {
                    v1.type_of()
                },
            }),
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

            (lhs, rhs) => return Err($self.error(ErrorKind::Incomparable {
                lhs: lhs.type_of(),
                rhs: rhs.type_of(),
            })),
        };

        $self.push(Value::Bool(result));
    }};
}

impl VM {
    fn frame(&self) -> &CallFrame {
        &self.frames[self.frames.len() - 1]
    }

    fn chunk(&self) -> &Chunk {
        &self.frame().lambda.chunk
    }

    fn frame_mut(&mut self) -> &mut CallFrame {
        let idx = self.frames.len() - 1;
        &mut self.frames[idx]
    }

    fn inc_ip(&mut self) -> OpCode {
        let op = self.chunk().code[self.frame().ip];
        self.frame_mut().ip += 1;
        op
    }

    fn peek_op(&self) -> OpCode {
        self.chunk().code[self.frame().ip]
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

    /// Returns the source span of the instruction currently being
    /// executed.
    fn current_span(&self) -> codemap::Span {
        self.chunk().get_span(CodeIdx(self.frame().ip - 1))
    }

    /// Construct an error from the given ErrorKind and the source
    /// span of the current instruction.
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            kind,
            span: self.current_span(),
        }
    }

    /// Execute the given lambda in this VM's context, returning its
    /// value after its stack frame completes.
    pub fn call(
        &mut self,
        lambda: Rc<Lambda>,
        upvalues: Vec<Value>,
        arg_count: usize,
    ) -> EvalResult<Value> {
        #[cfg(feature = "disassembler")]
        self.tracer.literal(&format!(
            "=== entering closure/{} [{}] ===",
            arg_count,
            self.frames.len()
        ));

        let frame = CallFrame {
            lambda,
            upvalues,
            ip: 0,
            stack_offset: self.stack.len() - arg_count,
        };

        self.frames.push(frame);
        let result = self.run();

        #[cfg(feature = "disassembler")]
        self.tracer.literal(&format!(
            "=== exiting closure/{} [{}] ===",
            arg_count,
            self.frames.len()
        ));

        result
    }

    /// Run the VM's current stack frame to completion and return the
    /// value.
    fn run(&mut self) -> EvalResult<Value> {
        loop {
            // Break the loop if this call frame has already run to
            // completion, pop it off, and return the value to the
            // caller.
            if self.frame().ip == self.chunk().code.len() {
                self.frames.pop();
                return Ok(self.pop());
            }

            let op = self.inc_ip();

            #[cfg(feature = "disassembler")]
            self.tracer.trace(&op, self.frame().ip, &self.stack);

            match op {
                OpCode::OpConstant(idx) => {
                    let c = self.chunk().constant(idx).clone();
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
                        fallible!(self, arithmetic_op!(a, b, +))
                    };

                    self.push(result)
                }

                OpCode::OpSub => arithmetic_op!(self, -),
                OpCode::OpMul => arithmetic_op!(self, *),
                OpCode::OpDiv => arithmetic_op!(self, /),

                OpCode::OpInvert => {
                    let v = fallible!(self, self.pop().as_bool());
                    self.push(Value::Bool(!v));
                }

                OpCode::OpNegate => match self.pop() {
                    Value::Integer(i) => self.push(Value::Integer(-i)),
                    Value::Float(f) => self.push(Value::Float(-f)),
                    v => {
                        return Err(self.error(ErrorKind::TypeError {
                            expected: "number (either int or float)",
                            actual: v.type_of(),
                        }));
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

                OpCode::OpAttrs(Count(count)) => self.run_attrset(count)?,
                OpCode::OpAttrPath(Count(count)) => self.run_attr_path(count)?,

                OpCode::OpAttrsUpdate => {
                    let rhs = unwrap_or_clone_rc(fallible!(self, self.pop().to_attrs()));
                    let lhs = unwrap_or_clone_rc(fallible!(self, self.pop().to_attrs()));

                    self.push(Value::Attrs(Rc::new(lhs.update(rhs))))
                }

                OpCode::OpAttrsSelect => {
                    let key = fallible!(self, self.pop().to_str());
                    let attrs = fallible!(self, self.pop().to_attrs());

                    match attrs.select(key.as_str()) {
                        Some(value) => self.push(value.clone()),

                        None => {
                            return Err(self.error(ErrorKind::AttributeNotFound {
                                name: key.as_str().to_string(),
                            }))
                        }
                    }
                }

                OpCode::OpAttrsTrySelect => {
                    let key = fallible!(self, self.pop().to_str());
                    let value = match self.pop() {
                        Value::Attrs(attrs) => match attrs.select(key.as_str()) {
                            Some(value) => value.clone(),
                            None => Value::AttrNotFound,
                        },

                        _ => Value::AttrNotFound,
                    };

                    self.push(value);
                }

                OpCode::OpAttrsIsSet => {
                    let key = fallible!(self, self.pop().to_str());
                    let result = match self.pop() {
                        Value::Attrs(attrs) => attrs.contains(key.as_str()),

                        // Nix allows use of `?` on non-set types, but
                        // always returns false in those cases.
                        _ => false,
                    };

                    self.push(Value::Bool(result));
                }

                OpCode::OpList(Count(count)) => {
                    let list =
                        NixList::construct(count, self.stack.split_off(self.stack.len() - count));
                    self.push(Value::List(list));
                }

                OpCode::OpConcat => {
                    let rhs = fallible!(self, self.pop().to_list());
                    let lhs = fallible!(self, self.pop().to_list());
                    self.push(Value::List(lhs.concat(&rhs)))
                }

                OpCode::OpInterpolate(Count(count)) => self.run_interpolate(count)?,

                OpCode::OpJump(JumpOffset(offset)) => {
                    self.frame_mut().ip += offset;
                }

                OpCode::OpJumpIfTrue(JumpOffset(offset)) => {
                    if fallible!(self, self.peek(0).as_bool()) {
                        self.frame_mut().ip += offset;
                    }
                }

                OpCode::OpJumpIfFalse(JumpOffset(offset)) => {
                    if !fallible!(self, self.peek(0).as_bool()) {
                        self.frame_mut().ip += offset;
                    }
                }

                OpCode::OpJumpIfNotFound(JumpOffset(offset)) => {
                    if matches!(self.peek(0), Value::AttrNotFound) {
                        self.pop();
                        self.frame_mut().ip += offset;
                    }
                }

                // These assertion operations error out if the stack
                // top is not of the expected type. This is necessary
                // to implement some specific behaviours of Nix
                // exactly.
                OpCode::OpAssertBool => {
                    let val = self.peek(0);
                    if !val.is_bool() {
                        return Err(self.error(ErrorKind::TypeError {
                            expected: "bool",
                            actual: val.type_of(),
                        }));
                    }
                }

                // Remove the given number of elements from the stack,
                // but retain the top value.
                OpCode::OpCloseScope(Count(count)) => {
                    // Immediately move the top value into the right
                    // position.
                    let target_idx = self.stack.len() - 1 - count;
                    self.stack[target_idx] = self.pop();

                    // Then drop the remaining values.
                    for _ in 0..(count - 1) {
                        self.pop();
                    }
                }

                OpCode::OpGetLocal(StackIdx(local_idx)) => {
                    let idx = self.frame().stack_offset + local_idx;
                    self.push(self.stack[idx].clone());
                }

                OpCode::OpPushWith(StackIdx(idx)) => {
                    self.with_stack.push(self.frame().stack_offset + idx)
                }

                OpCode::OpPopWith => {
                    self.with_stack.pop();
                }

                OpCode::OpResolveWith => {
                    let ident = fallible!(self, self.pop().to_str());
                    let value = self.resolve_with(ident.as_str())?;
                    self.push(value)
                }

                OpCode::OpResolveWithOrUpvalue(idx) => {
                    let ident = fallible!(self, self.pop().to_str());
                    match self.resolve_with(ident.as_str()) {
                        // Variable found in local `with`-stack.
                        Ok(value) => self.push(value),

                        // Variable not found => check upvalues.
                        Err(Error {
                            kind: ErrorKind::UnknownDynamicVariable(_),
                            ..
                        }) => {
                            let value = self.frame().upvalue(idx).clone();
                            self.push(value);
                        }

                        Err(err) => return Err(err),
                    }
                }

                OpCode::OpAssert => {
                    if !fallible!(self, self.pop().as_bool()) {
                        return Err(self.error(ErrorKind::AssertionFailed));
                    }
                }

                OpCode::OpCall => {
                    let callable = self.pop();
                    match callable {
                        Value::Closure(closure) => {
                            let result =
                                self.call(closure.lambda(), closure.upvalues().to_vec(), 1)?;
                            self.push(result)
                        }

                        Value::Builtin(builtin) => {
                            #[cfg(feature = "disassembler")]
                            self.tracer
                                .literal(&format!("=== entering builtins.{} ===", builtin.name()));

                            let arg = self.pop();
                            let result = fallible!(self, builtin.apply(self, arg));

                            #[cfg(feature = "disassembler")]
                            self.tracer.literal("=== exiting builtin ===");

                            self.push(result);
                        }
                        _ => return Err(self.error(ErrorKind::NotCallable)),
                    };
                }

                OpCode::OpGetUpvalue(upv_idx) => {
                    let value = self.frame().upvalue(upv_idx).clone();
                    if let Value::DynamicUpvalueMissing(name) = value {
                        return Err(self
                            .error(ErrorKind::UnknownDynamicVariable(name.as_str().to_string())));
                    }

                    self.push(value);
                }

                OpCode::OpClosure(idx) => {
                    let blueprint = match self.chunk().constant(idx) {
                        Value::Blueprint(lambda) => lambda.clone(),
                        _ => panic!("compiler bug: non-blueprint in blueprint slot"),
                    };

                    let upvalue_count = blueprint.upvalue_count;
                    debug_assert!(
                        upvalue_count > 0,
                        "OpClosure should not be called for plain lambdas"
                    );

                    let closure = Closure::new(blueprint);
                    let upvalues = closure.upvalues_mut();
                    self.push(Value::Closure(closure.clone()));

                    // From this point on we internally mutate the
                    // closure object's upvalues. The closure is
                    // already in its stack slot, which means that it
                    // can capture itself as an upvalue for
                    // self-recursion.
                    self.populate_upvalues(upvalue_count, upvalues)?;
                }

                OpCode::OpThunk(idx) => {
                    let blueprint = match self.chunk().constant(idx) {
                        Value::Blueprint(lambda) => lambda.clone(),
                        _ => panic!("compiler bug: non-blueprint in blueprint slot"),
                    };

                    let upvalue_count = blueprint.upvalue_count;
                    let thunk = Thunk::new(blueprint);
                    let upvalues = thunk.upvalues_mut();

                    self.push(Value::Thunk(thunk.clone()));
                    self.populate_upvalues(upvalue_count, upvalues)?;
                }

                OpCode::OpForce => {
                    let mut value = self.pop();

                    if let Value::Thunk(thunk) = value {
                        fallible!(self, thunk.force(self));
                        value = thunk.value().clone();
                    }

                    self.push(value);
                }

                OpCode::OpFinalise(StackIdx(idx)) => {
                    match &self.stack[self.frame().stack_offset + idx] {
                        Value::Closure(closure) => closure
                            .resolve_deferred_upvalues(&self.stack[self.frame().stack_offset..]),

                        Value::Thunk(thunk) => thunk
                            .resolve_deferred_upvalues(&self.stack[self.frame().stack_offset..]),

                        v => panic!("compiler error: invalid finaliser value: {}", v),
                    }
                }

                // Data-carrying operands should never be executed,
                // that is a critical error in the VM.
                OpCode::DataLocalIdx(_)
                | OpCode::DataDeferredLocal(_)
                | OpCode::DataUpvalueIdx(_)
                | OpCode::DataDynamicIdx(_)
                | OpCode::DataDynamicAncestor(_) => {
                    panic!("VM bug: attempted to execute data-carrying operand")
                }
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
            path.push(fallible!(self, self.pop().to_str()));
        }

        self.push(Value::AttrPath(path));
        Ok(())
    }

    fn run_attrset(&mut self, count: usize) -> EvalResult<()> {
        let attrs = fallible!(
            self,
            NixAttrs::construct(count, self.stack.split_off(self.stack.len() - count * 2))
        );

        self.push(Value::Attrs(Rc::new(attrs)));
        Ok(())
    }

    // Interpolate string fragments by popping the specified number of
    // fragments of the stack, evaluating them to strings, and pushing
    // the concatenated result string back on the stack.
    fn run_interpolate(&mut self, count: usize) -> EvalResult<()> {
        let mut out = String::new();

        for _ in 0..count {
            out.push_str(fallible!(self, self.pop().to_str()).as_str());
        }

        self.push(Value::String(out.into()));
        Ok(())
    }

    fn resolve_dynamic_upvalue(&mut self, ident_idx: ConstantIdx) -> EvalResult<Value> {
        let chunk = self.chunk();
        let ident = fallible!(self, chunk.constant(ident_idx).to_str());

        // Peek at the current instruction (note: IP has already
        // advanced!) to see if it is actually data indicating a
        // "fallback upvalue" in case the dynamic could not be
        // resolved at this level.
        let up = match self.peek_op() {
            OpCode::DataDynamicAncestor(idx) => {
                // advance ip past this data
                self.inc_ip();
                Some(idx)
            }
            _ => None,
        };

        match self.resolve_with(ident.as_str()) {
            Ok(v) => Ok(v),

            Err(Error {
                kind: ErrorKind::UnknownDynamicVariable(_),
                ..
            }) => match up {
                Some(idx) => Ok(self.frame().upvalue(idx).clone()),
                None => Ok(Value::DynamicUpvalueMissing(ident.into())),
            },

            Err(err) => Err(err),
        }
    }

    /// Resolve a dynamic identifier through the with-stack at runtime.
    fn resolve_with(&self, ident: &str) -> EvalResult<Value> {
        for idx in self.with_stack.iter().rev() {
            let with = fallible!(self, self.stack[*idx].to_attrs());
            match with.select(ident) {
                None => continue,
                Some(val) => return Ok(val.clone()),
            }
        }

        Err(self.error(ErrorKind::UnknownDynamicVariable(ident.to_string())))
    }

    /// Populate the upvalue fields of a thunk or closure under construction.
    fn populate_upvalues(
        &mut self,
        count: usize,
        mut upvalues: RefMut<'_, Vec<Value>>,
    ) -> EvalResult<()> {
        for _ in 0..count {
            match self.inc_ip() {
                OpCode::DataLocalIdx(StackIdx(local_idx)) => {
                    let idx = self.frame().stack_offset + local_idx;
                    upvalues.push(self.stack[idx].clone());
                }

                OpCode::DataUpvalueIdx(upv_idx) => {
                    upvalues.push(self.frame().upvalue(upv_idx).clone());
                }

                OpCode::DataDynamicIdx(ident_idx) => {
                    let value = self.resolve_dynamic_upvalue(ident_idx)?;
                    upvalues.push(value);
                }

                OpCode::DataDeferredLocal(idx) => {
                    upvalues.push(Value::DeferredUpvalue(idx));
                }

                _ => panic!("compiler error: missing closure operand"),
            }
        }

        Ok(())
    }

    /// Strictly evaluate the supplied value for outputting it. This
    /// will ensure that lists and attribute sets do not contain
    /// chunks which, for users, are displayed in a strange and often
    /// unexpected way.
    fn force_for_output(&mut self, value: &Value) -> EvalResult<()> {
        match value {
            Value::Attrs(attrs) => {
                for (_, value) in attrs.iter() {
                    self.force_for_output(value)?;
                }
                Ok(())
            }

            Value::List(list) => list.iter().try_for_each(|elem| self.force_for_output(elem)),

            Value::Thunk(thunk) => {
                fallible!(self, thunk.force(self));
                self.force_for_output(&thunk.value())
            }

            // If any of these internal values are encountered here a
            // critical error has happened (likely a compiler bug).
            Value::AttrPath(_)
            | Value::AttrNotFound
            | Value::DynamicUpvalueMissing(_)
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_) => {
                panic!("tvix bug: internal value left on stack: {:?}", value)
            }

            _ => Ok(()),
        }
    }

    pub fn new() -> Self {
        VM {
            frames: vec![],
            stack: vec![],
            with_stack: vec![],

            #[cfg(feature = "disassembler")]
            tracer: crate::disassembler::Tracer::new(),
        }
    }
}

// TODO: use Rc::unwrap_or_clone once it is stabilised.
// https://doc.rust-lang.org/std/rc/struct.Rc.html#method.unwrap_or_clone
fn unwrap_or_clone_rc<T: Clone>(rc: Rc<T>) -> T {
    Rc::try_unwrap(rc).unwrap_or_else(|rc| (*rc).clone())
}

pub fn run_lambda(lambda: Lambda) -> EvalResult<Value> {
    let mut vm = VM::new();
    let value = vm.call(Rc::new(lambda), vec![], 0)?;
    vm.force_for_output(&value)?;
    Ok(value)
}
