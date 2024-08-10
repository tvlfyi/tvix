//! This module implements the abstract/virtual machine that runs Tvix
//! bytecode.
//!
//! The operation of the VM is facilitated by the [`Frame`] type,
//! which controls the current execution state of the VM and is
//! processed within the VM's operating loop.
//!
//! A [`VM`] is used by instantiating it with an initial [`Frame`],
//! then triggering its execution and waiting for the VM to return or
//! yield an error.

pub mod generators;
mod macros;

use bstr::{BString, ByteSlice, ByteVec};
use codemap::Span;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::{cmp::Ordering, ops::DerefMut, path::PathBuf, rc::Rc};

use crate::{
    arithmetic_op,
    chunk::Chunk,
    cmp_op,
    compiler::GlobalsMap,
    errors::{CatchableErrorKind, Error, ErrorKind, EvalResult},
    io::EvalIO,
    lifted_pop,
    nix_search_path::NixSearchPath,
    observer::RuntimeObserver,
    opcode::{CodeIdx, Op, Position, UpvalueIdx},
    upvalues::Upvalues,
    value::{
        Builtin, BuiltinResult, Closure, CoercionKind, Lambda, NixAttrs, NixContext, NixList,
        PointerEquality, Thunk, Value,
    },
    vm::generators::GenCo,
    warnings::{EvalWarning, WarningKind},
    NixString, SourceCode,
};

use generators::{call_functor, Generator, GeneratorState};

use self::generators::{VMRequest, VMResponse};

/// Internal helper trait for taking a span from a variety of types, to make use
/// of `WithSpan` (defined below) more ergonomic at call sites.
trait GetSpan {
    fn get_span(self) -> Span;
}

impl<'o, IO> GetSpan for &VM<'o, IO> {
    fn get_span(self) -> Span {
        self.reasonable_span
    }
}

impl GetSpan for &CallFrame {
    fn get_span(self) -> Span {
        self.current_span()
    }
}

impl GetSpan for &Span {
    fn get_span(self) -> Span {
        *self
    }
}

impl GetSpan for Span {
    fn get_span(self) -> Span {
        self
    }
}

/// Internal helper trait for ergonomically converting from a `Result<T,
/// ErrorKind>` to a `Result<T, Error>` using the current span of a call frame,
/// and chaining the VM's frame stack around it for printing a cause chain.
trait WithSpan<T, S: GetSpan, IO> {
    fn with_span(self, top_span: S, vm: &VM<IO>) -> Result<T, Error>;
}

impl<T, S: GetSpan, IO> WithSpan<T, S, IO> for Result<T, ErrorKind> {
    fn with_span(self, top_span: S, vm: &VM<IO>) -> Result<T, Error> {
        match self {
            Ok(something) => Ok(something),
            Err(kind) => {
                let mut error = Error::new(kind, top_span.get_span(), vm.source.clone());

                // Wrap the top-level error in chaining errors for each element
                // of the frame stack.
                for frame in vm.frames.iter().rev() {
                    match frame {
                        Frame::CallFrame { span, .. } => {
                            error = Error::new(
                                ErrorKind::BytecodeError(Box::new(error)),
                                *span,
                                vm.source.clone(),
                            );
                        }
                        Frame::Generator { name, span, .. } => {
                            error = Error::new(
                                ErrorKind::NativeError {
                                    err: Box::new(error),
                                    gen_type: name,
                                },
                                *span,
                                vm.source.clone(),
                            );
                        }
                    }
                }

                Err(error)
            }
        }
    }
}

struct CallFrame {
    /// The lambda currently being executed.
    lambda: Rc<Lambda>,

    /// Optional captured upvalues of this frame (if a thunk or
    /// closure if being evaluated).
    upvalues: Rc<Upvalues>,

    /// Instruction pointer to the instruction currently being
    /// executed.
    ip: CodeIdx,

    /// Stack offset, i.e. the frames "view" into the VM's full stack.
    stack_offset: usize,
}

impl CallFrame {
    /// Retrieve an upvalue from this frame at the given index.
    fn upvalue(&self, idx: UpvalueIdx) -> &Value {
        &self.upvalues[idx]
    }

    /// Borrow the chunk of this frame's lambda.
    fn chunk(&self) -> &Chunk {
        &self.lambda.chunk
    }

    /// Increment this frame's instruction pointer and return the operation that
    /// the pointer moved past.
    fn inc_ip(&mut self) -> Op {
        debug_assert!(
            self.ip.0 < self.chunk().code.len(),
            "out of bounds code at IP {} in {:p}",
            self.ip.0,
            self.lambda
        );

        let op = self.chunk().code[self.ip.0];
        self.ip += 1;
        op.into()
    }

    /// Read a varint-encoded operand and return it. The frame pointer is
    /// incremented internally.
    fn read_uvarint(&mut self) -> u64 {
        let (arg, size) = self.chunk().read_uvarint(self.ip.0);
        self.ip += size;
        arg
    }

    /// Read a fixed-size u16 and increment the frame pointer.
    fn read_u16(&mut self) -> u16 {
        let arg = self.chunk().read_u16(self.ip.0);
        self.ip += 2;
        arg
    }

    /// Construct an error result from the given ErrorKind and the source span
    /// of the current instruction.
    pub fn error<T, IO>(&self, vm: &VM<IO>, kind: ErrorKind) -> Result<T, Error> {
        Err(kind).with_span(self, vm)
    }

    /// Returns the current span. This is potentially expensive and should only
    /// be used when actually constructing an error or warning.
    pub fn current_span(&self) -> Span {
        self.chunk().get_span(self.ip - 1)
    }
}

/// A frame represents an execution state of the VM. The VM has a stack of
/// frames representing the nesting of execution inside of the VM, and operates
/// on the frame at the top.
///
/// When a frame has been fully executed, it is removed from the VM's frame
/// stack and expected to leave a result [`Value`] on the top of the stack.
enum Frame {
    /// CallFrame represents the execution of Tvix bytecode within a thunk,
    /// function or closure.
    CallFrame {
        /// The call frame itself, separated out into another type to pass it
        /// around easily.
        call_frame: CallFrame,

        /// Span from which the call frame was launched.
        span: Span,
    },

    /// Generator represents a frame that can yield further
    /// instructions to the VM while its execution is being driven.
    ///
    /// A generator is essentially an asynchronous function that can
    /// be suspended while waiting for the VM to do something (e.g.
    /// thunk forcing), and resume at the same point.
    Generator {
        /// human-readable description of the generator,
        name: &'static str,

        /// Span from which the generator was launched.
        span: Span,

        state: GeneratorState,

        /// Generator itself, which can be resumed with `.resume()`.
        generator: Generator,
    },
}

impl Frame {
    pub fn span(&self) -> Span {
        match self {
            Frame::CallFrame { span, .. } | Frame::Generator { span, .. } => *span,
        }
    }
}

#[derive(Default)]
struct ImportCache(FxHashMap<PathBuf, Value>);

/// The `ImportCache` holds the `Value` resulting from `import`ing a certain
/// file, so that the same file doesn't need to be re-evaluated multiple times.
/// Currently the real path of the imported file (determined using
/// [`std::fs::canonicalize()`], not to be confused with our
/// [`crate::value::canon_path()`]) is used to identify the file,
/// just like C++ Nix does.
///
/// Errors while determining the real path are currently just ignored, since we
/// pass around some fake paths like `/__corepkgs__/fetchurl.nix`.
///
/// In the future, we could use something more sophisticated, like file hashes.
/// However, a consideration is that the eval cache is observable via impurities
/// like pointer equality and `builtins.trace`.
impl ImportCache {
    fn get(&self, path: PathBuf) -> Option<&Value> {
        let path = match std::fs::canonicalize(path.as_path()).map_err(ErrorKind::from) {
            Ok(path) => path,
            Err(_) => path,
        };
        self.0.get(&path)
    }

    fn insert(&mut self, path: PathBuf, value: Value) -> Option<Value> {
        self.0.insert(
            match std::fs::canonicalize(path.as_path()).map_err(ErrorKind::from) {
                Ok(path) => path,
                Err(_) => path,
            },
            value,
        )
    }
}

struct VM<'o, IO> {
    /// VM's frame stack, representing the execution contexts the VM is working
    /// through. Elements are usually pushed when functions are called, or
    /// thunks are being forced.
    frames: Vec<Frame>,

    /// The VM's top-level value stack. Within this stack, each code-executing
    /// frame holds a "view" of the stack representing the slice of the
    /// top-level stack that is relevant to its operation. This is done to avoid
    /// allocating a new `Vec` for each frame's stack.
    pub(crate) stack: Vec<Value>,

    /// Stack indices (absolute indexes into `stack`) of attribute
    /// sets from which variables should be dynamically resolved
    /// (`with`).
    with_stack: Vec<usize>,

    /// Runtime warnings collected during evaluation.
    warnings: Vec<EvalWarning>,

    /// Import cache, mapping absolute file paths to the value that
    /// they compile to. Note that this reuses thunks, too!
    // TODO: should probably be based on a file hash
    pub import_cache: ImportCache,

    /// Data structure holding all source code evaluated in this VM,
    /// used for pretty error reporting.
    source: SourceCode,

    /// Parsed Nix search path, which is used to resolve `<...>`
    /// references.
    nix_search_path: NixSearchPath,

    /// Implementation of I/O operations used for impure builtins and
    /// features like `import`.
    io_handle: IO,

    /// Runtime observer which can print traces of runtime operations.
    observer: &'o mut dyn RuntimeObserver,

    /// Strong reference to the globals, guaranteeing that they are
    /// kept alive for the duration of evaluation.
    ///
    /// This is important because recursive builtins (specifically
    /// `import`) hold a weak reference to the builtins, while the
    /// original strong reference is held by the compiler which does
    /// not exist anymore at runtime.
    #[allow(dead_code)]
    globals: Rc<GlobalsMap>,

    /// A reasonably applicable span that can be used for errors in each
    /// execution situation.
    ///
    /// The VM should update this whenever control flow changes take place (i.e.
    /// entering or exiting a frame to yield control somewhere).
    reasonable_span: Span,

    /// This field is responsible for handling `builtins.tryEval`. When that
    /// builtin is encountered, it sends a special message to the VM which
    /// pushes the frame index that requested to be informed of catchable
    /// errors in this field.
    ///
    /// The frame stack is then laid out like this:
    ///
    /// ```notrust
    /// ┌──┬──────────────────────────┐
    /// │ 0│ `Result`-producing frame │
    /// ├──┼──────────────────────────┤
    /// │-1│ `builtins.tryEval` frame │
    /// ├──┼──────────────────────────┤
    /// │..│ ... other frames ...     │
    /// └──┴──────────────────────────┘
    /// ```
    ///
    /// Control is yielded to the outer VM loop, which evaluates the next frame
    /// and returns the result itself to the `builtins.tryEval` frame.
    try_eval_frames: Vec<usize>,
}

impl<'o, IO> VM<'o, IO>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    pub fn new(
        nix_search_path: NixSearchPath,
        io_handle: IO,
        observer: &'o mut dyn RuntimeObserver,
        source: SourceCode,
        globals: Rc<GlobalsMap>,
        reasonable_span: Span,
    ) -> Self {
        Self {
            nix_search_path,
            io_handle,
            observer,
            globals,
            reasonable_span,
            source,
            frames: vec![],
            stack: vec![],
            with_stack: vec![],
            warnings: vec![],
            import_cache: Default::default(),
            try_eval_frames: vec![],
        }
    }

    /// Push a call frame onto the frame stack.
    fn push_call_frame(&mut self, span: Span, call_frame: CallFrame) {
        self.frames.push(Frame::CallFrame { span, call_frame })
    }

    /// Run the VM's primary (outer) execution loop, continuing execution based
    /// on the current frame at the top of the frame stack.
    fn execute(mut self) -> EvalResult<RuntimeResult> {
        while let Some(frame) = self.frames.pop() {
            self.reasonable_span = frame.span();
            let frame_id = self.frames.len();

            match frame {
                Frame::CallFrame { call_frame, span } => {
                    self.observer
                        .observe_enter_call_frame(0, &call_frame.lambda, frame_id);

                    match self.execute_bytecode(span, call_frame) {
                        Ok(true) => self.observer.observe_exit_call_frame(frame_id, &self.stack),
                        Ok(false) => self
                            .observer
                            .observe_suspend_call_frame(frame_id, &self.stack),

                        Err(err) => return Err(err),
                    };
                }

                // Handle generator frames, which can request thunk forcing
                // during their execution.
                Frame::Generator {
                    name,
                    span,
                    state,
                    generator,
                } => {
                    self.observer
                        .observe_enter_generator(frame_id, name, &self.stack);

                    match self.run_generator(name, span, frame_id, state, generator, None) {
                        Ok(true) => {
                            self.observer
                                .observe_exit_generator(frame_id, name, &self.stack)
                        }
                        Ok(false) => {
                            self.observer
                                .observe_suspend_generator(frame_id, name, &self.stack)
                        }

                        Err(err) => return Err(err),
                    };
                }
            }
        }

        // Once no more frames are present, return the stack's top value as the
        // result.
        let value = self
            .stack
            .pop()
            .expect("tvix bug: runtime stack empty after execution");
        Ok(RuntimeResult {
            value,
            warnings: self.warnings,
        })
    }

    /// Run the VM's inner execution loop, processing Tvix bytecode from a
    /// chunk. This function returns if:
    ///
    /// 1. The code has run to the end, and has left a value on the top of the
    ///    stack. In this case, the frame is not returned to the frame stack.
    ///
    /// 2. The code encounters a generator, in which case the frame in its
    /// current state is pushed back on the stack, and the generator is left on
    /// top of it for the outer loop to execute.
    ///
    /// 3. An error is encountered.
    ///
    /// This function *must* ensure that it leaves the frame stack in the
    /// correct order, especially when re-enqueuing a frame to execute.
    ///
    /// The return value indicates whether the bytecode has been executed to
    /// completion, or whether it has been suspended in favour of a generator.
    fn execute_bytecode(&mut self, span: Span, mut frame: CallFrame) -> EvalResult<bool> {
        loop {
            let op = frame.inc_ip();
            self.observer.observe_execute_op(frame.ip, &op, &self.stack);

            match op {
                Op::ThunkSuspended | Op::ThunkClosure => {
                    let idx = frame.read_uvarint() as usize;

                    let blueprint = match &frame.chunk().constants[idx] {
                        Value::Blueprint(lambda) => lambda.clone(),
                        _ => panic!("compiler bug: non-blueprint in blueprint slot"),
                    };

                    let upvalue_count = frame.read_uvarint();

                    debug_assert!(
                        (upvalue_count >> 1) == blueprint.upvalue_count as u64,
                        "TODO: new upvalue count not correct",
                    );

                    let thunk = if op == Op::ThunkClosure {
                        debug_assert!(
                            (((upvalue_count >> 1) > 0) || (upvalue_count & 0b1 == 1)),
                            "OpThunkClosure should not be called for plain lambdas",
                        );
                        Thunk::new_closure(blueprint)
                    } else {
                        Thunk::new_suspended(blueprint, frame.current_span())
                    };
                    let upvalues = thunk.upvalues_mut();
                    self.stack.push(Value::Thunk(thunk.clone()));

                    // From this point on we internally mutate the
                    // upvalues. The closure (if `is_closure`) is
                    // already in its stack slot, which means that it
                    // can capture itself as an upvalue for
                    // self-recursion.
                    self.populate_upvalues(&mut frame, upvalue_count, upvalues)?;
                }

                Op::Force => {
                    if let Some(Value::Thunk(_)) = self.stack.last() {
                        let thunk = match self.stack_pop() {
                            Value::Thunk(t) => t,
                            _ => unreachable!(),
                        };

                        let gen_span = frame.current_span();

                        self.push_call_frame(span, frame);
                        self.enqueue_generator("force", gen_span, |co| {
                            Thunk::force(thunk, co, gen_span)
                        });

                        return Ok(false);
                    }
                }

                Op::GetUpvalue => {
                    let idx = UpvalueIdx(frame.read_uvarint() as usize);
                    let value = frame.upvalue(idx).clone();
                    self.stack.push(value);
                }

                // Discard the current frame.
                Op::Return => {
                    // TODO(amjoseph): I think this should assert `==` rather
                    // than `<=` but it fails with the stricter condition.
                    debug_assert!(self.stack.len() - 1 <= frame.stack_offset);
                    return Ok(true);
                }

                Op::Constant => {
                    let idx = frame.read_uvarint() as usize;

                    debug_assert!(
                        idx < frame.chunk().constants.len(),
                        "out of bounds constant at IP {} in {:p}",
                        frame.ip.0,
                        frame.lambda
                    );

                    let c = frame.chunk().constants[idx].clone();
                    self.stack.push(c);
                }

                Op::Call => {
                    let callable = self.stack_pop();
                    self.call_value(frame.current_span(), Some((span, frame)), callable)?;

                    // exit this loop and let the outer loop enter the new call
                    return Ok(true);
                }

                // Remove the given number of elements from the stack,
                // but retain the top value.
                Op::CloseScope => {
                    let count = frame.read_uvarint() as usize;
                    // Immediately move the top value into the right
                    // position.
                    let target_idx = self.stack.len() - 1 - count;
                    self.stack[target_idx] = self.stack_pop();

                    // Then drop the remaining values.
                    for _ in 0..(count - 1) {
                        self.stack.pop();
                    }
                }

                Op::Closure => {
                    let idx = frame.read_uvarint() as usize;
                    let blueprint = match &frame.chunk().constants[idx] {
                        Value::Blueprint(lambda) => lambda.clone(),
                        _ => panic!("compiler bug: non-blueprint in blueprint slot"),
                    };

                    let upvalue_count = frame.read_uvarint();

                    debug_assert!(
                        (upvalue_count >> 1) == blueprint.upvalue_count as u64,
                        "TODO: new upvalue count not correct in closure",
                    );

                    debug_assert!(
                        ((upvalue_count >> 1) > 0 || (upvalue_count & 0b1 == 1)),
                        "OpClosure should not be called for plain lambdas"
                    );

                    let mut upvalues = Upvalues::with_capacity(blueprint.upvalue_count);
                    self.populate_upvalues(&mut frame, upvalue_count, &mut upvalues)?;
                    self.stack
                        .push(Value::Closure(Rc::new(Closure::new_with_upvalues(
                            Rc::new(upvalues),
                            blueprint,
                        ))));
                }

                Op::AttrsSelect => lifted_pop! {
                    self(key, attrs) => {
                        let key = key.to_str().with_span(&frame, self)?;
                        let attrs = attrs.to_attrs().with_span(&frame, self)?;

                        match attrs.select(&key) {
                            Some(value) => self.stack.push(value.clone()),

                            None => {
                                return frame.error(
                                    self,
                                    ErrorKind::AttributeNotFound {
                                        name: key.to_str_lossy().into_owned()
                                    },
                                );
                            }
                        }
                    }
                },

                Op::JumpIfFalse => {
                    let offset = frame.read_u16() as usize;
                    debug_assert!(offset != 0);
                    if !self.stack_peek(0).as_bool().with_span(&frame, self)? {
                        frame.ip += offset;
                    }
                }

                Op::JumpIfCatchable => {
                    let offset = frame.read_u16() as usize;
                    debug_assert!(offset != 0);
                    if self.stack_peek(0).is_catchable() {
                        frame.ip += offset;
                    }
                }

                Op::JumpIfNoFinaliseRequest => {
                    let offset = frame.read_u16() as usize;
                    debug_assert!(offset != 0);
                    match self.stack_peek(0) {
                        Value::FinaliseRequest(finalise) => {
                            if !finalise {
                                frame.ip += offset;
                            }
                        },
                        val => panic!("Tvix bug: OpJumIfNoFinaliseRequest: expected FinaliseRequest, but got {}", val.type_of()),
                    }
                }

                Op::Pop => {
                    self.stack.pop();
                }

                Op::AttrsTrySelect => {
                    let key = self.stack_pop().to_str().with_span(&frame, self)?;
                    let value = match self.stack_pop() {
                        Value::Attrs(attrs) => match attrs.select(&key) {
                            Some(value) => value.clone(),
                            None => Value::AttrNotFound,
                        },

                        _ => Value::AttrNotFound,
                    };

                    self.stack.push(value);
                }

                Op::GetLocal => {
                    let local_idx = frame.read_uvarint() as usize;
                    let idx = frame.stack_offset + local_idx;
                    self.stack.push(self.stack[idx].clone());
                }

                Op::JumpIfNotFound => {
                    let offset = frame.read_u16() as usize;
                    debug_assert!(offset != 0);
                    if matches!(self.stack_peek(0), Value::AttrNotFound) {
                        self.stack_pop();
                        frame.ip += offset;
                    }
                }

                Op::Jump => {
                    let offset = frame.read_u16() as usize;
                    debug_assert!(offset != 0);
                    frame.ip += offset;
                }

                Op::Equal => lifted_pop! {
                    self(b, a) => {
                        let gen_span = frame.current_span();
                        self.push_call_frame(span, frame);
                        self.enqueue_generator("nix_eq", gen_span, |co| {
                            a.nix_eq_owned_genco(b, co, PointerEquality::ForbidAll, gen_span)
                        });
                        return Ok(false);
                    }
                },

                // These assertion operations error out if the stack
                // top is not of the expected type. This is necessary
                // to implement some specific behaviours of Nix
                // exactly.
                Op::AssertBool => {
                    let val = self.stack_peek(0);
                    // TODO(edef): propagate this into is_bool, since bottom values *are* values of any type
                    if !val.is_catchable() && !val.is_bool() {
                        return frame.error(
                            self,
                            ErrorKind::TypeError {
                                expected: "bool",
                                actual: val.type_of(),
                            },
                        );
                    }
                }

                Op::AssertAttrs => {
                    let val = self.stack_peek(0);
                    // TODO(edef): propagate this into is_attrs, since bottom values *are* values of any type
                    if !val.is_catchable() && !val.is_attrs() {
                        return frame.error(
                            self,
                            ErrorKind::TypeError {
                                expected: "set",
                                actual: val.type_of(),
                            },
                        );
                    }
                }

                Op::Attrs => self.run_attrset(frame.read_uvarint() as usize, &frame)?,

                Op::AttrsUpdate => lifted_pop! {
                    self(rhs, lhs) => {
                        let rhs = rhs.to_attrs().with_span(&frame, self)?;
                        let lhs = lhs.to_attrs().with_span(&frame, self)?;
                        self.stack.push(Value::attrs(lhs.update(*rhs)))
                    }
                },

                Op::Invert => lifted_pop! {
                    self(v) => {
                        let v = v.as_bool().with_span(&frame, self)?;
                        self.stack.push(Value::Bool(!v));
                    }
                },

                Op::List => {
                    let count = frame.read_uvarint() as usize;
                    let list =
                        NixList::construct(count, self.stack.split_off(self.stack.len() - count));

                    self.stack.push(Value::List(list));
                }

                Op::JumpIfTrue => {
                    let offset = frame.read_u16() as usize;
                    debug_assert!(offset != 0);
                    if self.stack_peek(0).as_bool().with_span(&frame, self)? {
                        frame.ip += offset;
                    }
                }

                Op::HasAttr => lifted_pop! {
                    self(key, attrs) => {
                        let key = key.to_str().with_span(&frame, self)?;
                        let result = match attrs {
                            Value::Attrs(attrs) => attrs.contains(&key),

                            // Nix allows use of `?` on non-set types, but
                            // always returns false in those cases.
                            _ => false,
                        };

                        self.stack.push(Value::Bool(result));
                    }
                },

                Op::Concat => lifted_pop! {
                    self(rhs, lhs) => {
                        let rhs = rhs.to_list().with_span(&frame, self)?.into_inner();
                        let lhs = lhs.to_list().with_span(&frame, self)?.into_inner();
                        self.stack.push(Value::List(NixList::from(lhs + rhs)))
                    }
                },

                Op::ResolveWith => {
                    let ident = self.stack_pop().to_str().with_span(&frame, self)?;

                    // Re-enqueue this frame.
                    let op_span = frame.current_span();
                    self.push_call_frame(span, frame);

                    // Construct a generator frame doing the lookup in constant
                    // stack space.
                    let with_stack_len = self.with_stack.len();
                    let closed_with_stack_len = self
                        .last_call_frame()
                        .map(|frame| frame.upvalues.with_stack_len())
                        .unwrap_or(0);

                    self.enqueue_generator("resolve_with", op_span, |co| {
                        resolve_with(
                            co,
                            ident.as_bstr().to_owned(),
                            with_stack_len,
                            closed_with_stack_len,
                        )
                    });

                    return Ok(false);
                }

                Op::Finalise => {
                    let idx = frame.read_uvarint() as usize;
                    match &self.stack[frame.stack_offset + idx] {
                        Value::Closure(_) => panic!("attempted to finalise a closure"),
                        Value::Thunk(thunk) => thunk.finalise(&self.stack[frame.stack_offset..]),
                        _ => panic!("attempted to finalise a non-thunk"),
                    }
                }

                Op::CoerceToString => {
                    let kind: CoercionKind = frame.chunk().code[frame.ip.0].into();
                    frame.ip.0 += 1;

                    let value = self.stack_pop();
                    let gen_span = frame.current_span();
                    self.push_call_frame(span, frame);

                    self.enqueue_generator("coerce_to_string", gen_span, |co| {
                        value.coerce_to_string(co, kind, gen_span)
                    });

                    return Ok(false);
                }

                Op::Interpolate => self.run_interpolate(frame.read_uvarint(), &frame)?,

                Op::ValidateClosedFormals => {
                    let formals = frame.lambda.formals.as_ref().expect(
                        "OpValidateClosedFormals called within the frame of a lambda without formals",
                    );

                    let peeked = self.stack_peek(0);
                    if peeked.is_catchable() {
                        continue;
                    }

                    let args = peeked.to_attrs().with_span(&frame, self)?;
                    for arg in args.keys() {
                        if !formals.contains(arg) {
                            return frame.error(
                                self,
                                ErrorKind::UnexpectedArgumentFormals {
                                    arg: arg.clone(),
                                    formals_span: formals.span,
                                },
                            );
                        }
                    }
                }

                Op::Add => lifted_pop! {
                    self(b, a) => {
                        let gen_span = frame.current_span();
                        self.push_call_frame(span, frame);

                        // OpAdd can add not just numbers, but also string-like
                        // things, which requires more VM logic. This operation is
                        // evaluated in a generator frame.
                        self.enqueue_generator("add_values", gen_span, |co| add_values(co, a, b));
                        return Ok(false);
                    }
                },

                Op::Sub => lifted_pop! {
                    self(b, a) => {
                        let result = arithmetic_op!(&a, &b, -).with_span(&frame, self)?;
                        self.stack.push(result);
                    }
                },

                Op::Mul => lifted_pop! {
                    self(b, a) => {
                        let result = arithmetic_op!(&a, &b, *).with_span(&frame, self)?;
                        self.stack.push(result);
                    }
                },

                Op::Div => lifted_pop! {
                    self(b, a) => {
                        match b {
                            Value::Integer(0) => return frame.error(self, ErrorKind::DivisionByZero),
                            Value::Float(b) if b == 0.0_f64 => {
                                return frame.error(self, ErrorKind::DivisionByZero)
                            }
                            _ => {}
                        };

                        let result = arithmetic_op!(&a, &b, /).with_span(&frame, self)?;
                        self.stack.push(result);
                    }
                },

                Op::Negate => match self.stack_pop() {
                    Value::Integer(i) => self.stack.push(Value::Integer(-i)),
                    Value::Float(f) => self.stack.push(Value::Float(-f)),
                    Value::Catchable(cex) => self.stack.push(Value::Catchable(cex)),
                    v => {
                        return frame.error(
                            self,
                            ErrorKind::TypeError {
                                expected: "number (either int or float)",
                                actual: v.type_of(),
                            },
                        );
                    }
                },

                Op::Less => cmp_op!(self, frame, span, <),
                Op::LessOrEq => cmp_op!(self, frame, span, <=),
                Op::More => cmp_op!(self, frame, span, >),
                Op::MoreOrEq => cmp_op!(self, frame, span, >=),

                Op::FindFile => match self.stack_pop() {
                    Value::UnresolvedPath(path) => {
                        let resolved = self
                            .nix_search_path
                            .resolve(&self.io_handle, *path)
                            .with_span(&frame, self)?;
                        self.stack.push(resolved.into());
                    }

                    _ => panic!("tvix compiler bug: OpFindFile called on non-UnresolvedPath"),
                },

                Op::ResolveHomePath => match self.stack_pop() {
                    Value::UnresolvedPath(path) => {
                        match dirs::home_dir() {
                            None => {
                                return frame.error(
                                    self,
                                    ErrorKind::RelativePathResolution(
                                        "failed to determine home directory".into(),
                                    ),
                                );
                            }
                            Some(mut buf) => {
                                buf.push(*path);
                                self.stack.push(buf.into());
                            }
                        };
                    }

                    _ => {
                        panic!("tvix compiler bug: OpResolveHomePath called on non-UnresolvedPath")
                    }
                },

                Op::PushWith => self
                    .with_stack
                    .push(frame.stack_offset + frame.read_uvarint() as usize),

                Op::PopWith => {
                    self.with_stack.pop();
                }

                Op::AssertFail => {
                    self.stack
                        .push(Value::from(CatchableErrorKind::AssertionFailed));
                }

                // Encountering an invalid opcode is a critical error in the
                // VM/compiler.
                Op::Invalid => {
                    panic!("Tvix bug: attempted to execute invalid opcode")
                }
            }
        }
    }
}

/// Implementation of helper functions for the runtime logic above.
impl<'o, IO> VM<'o, IO>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    pub(crate) fn stack_pop(&mut self) -> Value {
        self.stack.pop().expect("runtime stack empty")
    }

    fn stack_peek(&self, offset: usize) -> &Value {
        &self.stack[self.stack.len() - 1 - offset]
    }

    fn run_attrset(&mut self, count: usize, frame: &CallFrame) -> EvalResult<()> {
        let attrs = NixAttrs::construct(count, self.stack.split_off(self.stack.len() - count * 2))
            .with_span(frame, self)?
            .map(Value::attrs)
            .into();

        self.stack.push(attrs);
        Ok(())
    }

    /// Access the last call frame present in the frame stack.
    fn last_call_frame(&self) -> Option<&CallFrame> {
        for frame in self.frames.iter().rev() {
            if let Frame::CallFrame { call_frame, .. } = frame {
                return Some(call_frame);
            }
        }

        None
    }

    /// Push an already constructed warning.
    pub fn push_warning(&mut self, warning: EvalWarning) {
        self.warnings.push(warning);
    }

    /// Emit a warning with the given WarningKind and the source span
    /// of the current instruction.
    pub fn emit_warning(&mut self, kind: WarningKind) {
        self.push_warning(EvalWarning {
            kind,
            span: self.get_span(),
        });
    }

    /// Interpolate string fragments by popping the specified number of
    /// fragments of the stack, evaluating them to strings, and pushing
    /// the concatenated result string back on the stack.
    fn run_interpolate(&mut self, count: u64, frame: &CallFrame) -> EvalResult<()> {
        let mut out = BString::default();
        // Interpolation propagates the context and union them.
        let mut context: NixContext = NixContext::new();

        for i in 0..count {
            let val = self.stack_pop();
            if val.is_catchable() {
                for _ in (i + 1)..count {
                    self.stack.pop();
                }
                self.stack.push(val);
                return Ok(());
            }
            let mut nix_string = val.to_contextful_str().with_span(frame, self)?;
            out.push_str(nix_string.as_bstr());
            if let Some(nix_string_ctx) = nix_string.take_context() {
                context.extend(nix_string_ctx.into_iter())
            }
        }

        self.stack
            .push(Value::String(NixString::new_context_from(context, out)));
        Ok(())
    }

    /// Apply an argument from the stack to a builtin, and attempt to call it.
    ///
    /// All calls are tail-calls in Tvix, as every function application is a
    /// separate thunk and OpCall is thus the last result in the thunk.
    ///
    /// Due to this, once control flow exits this function, the generator will
    /// automatically be run by the VM.
    fn call_builtin(&mut self, span: Span, mut builtin: Builtin) -> EvalResult<()> {
        let builtin_name = builtin.name();
        self.observer.observe_enter_builtin(builtin_name);

        builtin.apply_arg(self.stack_pop());

        match builtin.call() {
            // Partially applied builtin is just pushed back on the stack.
            BuiltinResult::Partial(partial) => self.stack.push(Value::Builtin(partial)),

            // Builtin is fully applied and the generator needs to be run by the VM.
            BuiltinResult::Called(name, generator) => self.frames.push(Frame::Generator {
                generator,
                span,
                name,
                state: GeneratorState::Running,
            }),
        }

        Ok(())
    }

    fn call_value(
        &mut self,
        span: Span,
        parent: Option<(Span, CallFrame)>,
        callable: Value,
    ) -> EvalResult<()> {
        match callable {
            Value::Builtin(builtin) => self.call_builtin(span, builtin),
            Value::Thunk(thunk) => self.call_value(span, parent, thunk.value().clone()),

            Value::Closure(closure) => {
                let lambda = closure.lambda();
                self.observer.observe_tail_call(self.frames.len(), &lambda);

                // The stack offset is always `stack.len() - arg_count`, and
                // since this branch handles native Nix functions (which always
                // take only a single argument and are curried), the offset is
                // `stack_len - 1`.
                let stack_offset = self.stack.len() - 1;

                // Reenqueue the parent frame, which should only have
                // `OpReturn` left. Not throwing it away leads to more
                // useful error traces.
                if let Some((parent_span, parent_frame)) = parent {
                    self.push_call_frame(parent_span, parent_frame);
                }

                self.push_call_frame(
                    span,
                    CallFrame {
                        lambda,
                        upvalues: closure.upvalues(),
                        ip: CodeIdx(0),
                        stack_offset,
                    },
                );

                Ok(())
            }

            // Attribute sets with a __functor attribute are callable.
            val @ Value::Attrs(_) => {
                if let Some((parent_span, parent_frame)) = parent {
                    self.push_call_frame(parent_span, parent_frame);
                }

                self.enqueue_generator("__functor call", span, |co| call_functor(co, val));
                Ok(())
            }

            val @ Value::Catchable(_) => {
                // the argument that we tried to apply a catchable to
                self.stack.pop();
                // applying a `throw` to anything is still a `throw`, so we just
                // push it back on the stack.
                self.stack.push(val);
                Ok(())
            }

            v => Err(ErrorKind::NotCallable(v.type_of())).with_span(span, self),
        }
    }

    /// Populate the upvalue fields of a thunk or closure under construction.
    ///
    /// See the closely tied function `emit_upvalue_data` in the compiler
    /// implementation for details on the argument processing.
    fn populate_upvalues(
        &mut self,
        frame: &mut CallFrame,
        count: u64,
        mut upvalues: impl DerefMut<Target = Upvalues>,
    ) -> EvalResult<()> {
        // Determine whether to capture the with stack, and then shift the
        // actual count of upvalues back.
        let capture_with = count & 0b1 == 1;
        let count = count >> 1;
        if capture_with {
            // Start the captured with_stack off of the
            // current call frame's captured with_stack, ...
            let mut captured_with_stack = frame
                .upvalues
                .with_stack()
                .cloned()
                // ... or make an empty one if there isn't one already.
                .unwrap_or_else(|| Vec::with_capacity(self.with_stack.len()));

            for idx in &self.with_stack {
                captured_with_stack.push(self.stack[*idx].clone());
            }

            upvalues.deref_mut().set_with_stack(captured_with_stack);
        }

        for _ in 0..count {
            let pos = Position(frame.read_uvarint());

            if let Some(stack_idx) = pos.runtime_stack_index() {
                let idx = frame.stack_offset + stack_idx.0;

                let val = match self.stack.get(idx) {
                    Some(val) => val.clone(),
                    None => {
                        return frame.error(
                            self,
                            ErrorKind::TvixBug {
                                msg: "upvalue to be captured was missing on stack",
                                metadata: Some(Rc::new(json!({
                                    "ip": format!("{:#x}", frame.ip.0 - 1),
                                    "stack_idx(relative)": stack_idx.0,
                                    "stack_idx(absolute)": idx,
                                }))),
                            },
                        );
                    }
                };

                upvalues.deref_mut().push(val);
                continue;
            }

            if let Some(idx) = pos.runtime_deferred_local() {
                upvalues.deref_mut().push(Value::DeferredUpvalue(idx));
                continue;
            }

            if let Some(idx) = pos.runtime_upvalue_index() {
                upvalues.deref_mut().push(frame.upvalue(idx).clone());
                continue;
            }

            panic!("Tvix bug: invalid capture position emitted")
        }

        Ok(())
    }
}

// TODO(amjoseph): de-asyncify this
/// Resolve a dynamically bound identifier (through `with`) by looking
/// for matching values in the with-stacks carried at runtime.
async fn resolve_with(
    co: GenCo,
    ident: BString,
    vm_with_len: usize,
    upvalue_with_len: usize,
) -> Result<Value, ErrorKind> {
    /// Fetch and force a value on the with-stack from the VM.
    async fn fetch_forced_with(co: &GenCo, idx: usize) -> Value {
        match co.yield_(VMRequest::WithValue(idx)).await {
            VMResponse::Value(value) => value,
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        }
    }

    /// Fetch and force a value on the *captured* with-stack from the VM.
    async fn fetch_captured_with(co: &GenCo, idx: usize) -> Value {
        match co.yield_(VMRequest::CapturedWithValue(idx)).await {
            VMResponse::Value(value) => value,
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        }
    }

    for with_stack_idx in (0..vm_with_len).rev() {
        // TODO(tazjin): is this branch still live with the current with-thunking?
        let with = fetch_forced_with(&co, with_stack_idx).await;

        if with.is_catchable() {
            return Ok(with);
        }

        match with.to_attrs()?.select(&ident) {
            None => continue,
            Some(val) => return Ok(val.clone()),
        }
    }

    for upvalue_with_idx in (0..upvalue_with_len).rev() {
        let with = fetch_captured_with(&co, upvalue_with_idx).await;

        if with.is_catchable() {
            return Ok(with);
        }

        match with.to_attrs()?.select(&ident) {
            None => continue,
            Some(val) => return Ok(val.clone()),
        }
    }

    Err(ErrorKind::UnknownDynamicVariable(ident.to_string()))
}

// TODO(amjoseph): de-asyncify this
async fn add_values(co: GenCo, a: Value, b: Value) -> Result<Value, ErrorKind> {
    // What we try to do is solely determined by the type of the first value!
    let result = match (a, b) {
        (Value::Path(p), v) => {
            let mut path = p.into_os_string();
            match generators::request_string_coerce(
                &co,
                v,
                CoercionKind {
                    strong: false,

                    // Concatenating a Path with something else results in a
                    // Path, so we don't need to import any paths (paths
                    // imported by Nix always exist as a string, unless
                    // converted by the user). In C++ Nix they even may not
                    // contain any string context, the resulting error of such a
                    // case can not be replicated by us.
                    import_paths: false,
                    // FIXME(raitobezarius): per https://b.tvl.fyi/issues/364, this is a usecase
                    // for having a `reject_context: true` option here. This didn't occur yet in
                    // nixpkgs during my evaluations, therefore, I skipped it.
                },
            )
            .await
            {
                Ok(vs) => {
                    path.push(vs.to_os_str()?);
                    crate::value::canon_path(PathBuf::from(path)).into()
                }
                Err(c) => Value::Catchable(Box::new(c)),
            }
        }
        (Value::String(s1), Value::String(s2)) => Value::String(s1.concat(&s2)),
        (Value::String(s1), v) => generators::request_string_coerce(
            &co,
            v,
            CoercionKind {
                strong: false,
                // Behaves the same as string interpolation
                import_paths: true,
            },
        )
        .await
        .map(|s2| Value::String(s1.concat(&s2)))
        .into(),
        (a @ Value::Integer(_), b) | (a @ Value::Float(_), b) => arithmetic_op!(&a, &b, +)?,
        (a, b) => {
            let r1 = generators::request_string_coerce(
                &co,
                a,
                CoercionKind {
                    strong: false,
                    import_paths: false,
                },
            )
            .await;
            let r2 = generators::request_string_coerce(
                &co,
                b,
                CoercionKind {
                    strong: false,
                    import_paths: false,
                },
            )
            .await;
            match (r1, r2) {
                (Ok(s1), Ok(s2)) => Value::String(s1.concat(&s2)),
                (Err(c), _) => return Ok(Value::from(c)),
                (_, Err(c)) => return Ok(Value::from(c)),
            }
        }
    };

    Ok(result)
}

/// The result of a VM's runtime evaluation.
pub struct RuntimeResult {
    pub value: Value,
    pub warnings: Vec<EvalWarning>,
}

// TODO(amjoseph): de-asyncify this
/// Generator that retrieves the final value from the stack, and deep-forces it
/// before returning.
async fn final_deep_force(co: GenCo) -> Result<Value, ErrorKind> {
    let value = generators::request_stack_pop(&co).await;
    Ok(generators::request_deep_force(&co, value).await)
}

pub fn run_lambda<IO>(
    nix_search_path: NixSearchPath,
    io_handle: IO,
    observer: &mut dyn RuntimeObserver,
    source: SourceCode,
    globals: Rc<GlobalsMap>,
    lambda: Rc<Lambda>,
    strict: bool,
) -> EvalResult<RuntimeResult>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    // Retain the top-level span of the expression in this lambda, as
    // synthetic "calls" in deep_force will otherwise not have a span
    // to fall back to.
    //
    // We exploit the fact that the compiler emits a final instruction
    // with the span of the entire file for top-level expressions.
    let root_span = lambda.chunk.get_span(CodeIdx(lambda.chunk.code.len() - 1));

    let mut vm = VM::new(
        nix_search_path,
        io_handle,
        observer,
        source,
        globals,
        root_span,
    );

    // When evaluating strictly, synthesise a frame that will instruct
    // the VM to deep-force the final value before returning it.
    if strict {
        vm.enqueue_generator("final_deep_force", root_span, final_deep_force);
    }

    vm.frames.push(Frame::CallFrame {
        span: root_span,
        call_frame: CallFrame {
            lambda,
            upvalues: Rc::new(Upvalues::with_capacity(0)),
            ip: CodeIdx(0),
            stack_offset: 0,
        },
    });

    vm.execute()
}
