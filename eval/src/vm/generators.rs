//! This module implements generator logic for the VM. Generators are functions
//! used during evaluation which can suspend their execution during their
//! control flow, and request that the VM do something.
//!
//! This is used to keep the VM's stack size constant even when evaluating
//! deeply nested recursive data structures.
//!
//! We implement generators using the [`genawaiter`] crate.

use core::pin::Pin;
use genawaiter::rc::Co;
pub use genawaiter::rc::Gen;
use std::fmt::Display;
use std::future::Future;

use crate::value::PointerEquality;
use crate::warnings::{EvalWarning, WarningKind};
use crate::FileType;
use crate::NixString;

use super::*;

// -- Implementation of generic generator logic.

/// States that a generator can be in while being driven by the VM.
pub(crate) enum GeneratorState {
    /// Normal execution of the generator.
    Running,

    /// Generator is awaiting the result of a forced value.
    AwaitingValue,
}

/// Messages that can be sent from generators *to* the VM. In most
/// cases, the VM will suspend the generator when receiving a message
/// and enter some other frame to process the request.
///
/// Responses are returned to generators via the [`GeneratorResponse`] type.
pub enum VMRequest {
    /// Request that the VM forces this value. This message is first sent to the
    /// VM with the unforced value, then returned to the generator with the
    /// forced result.
    ForceValue(Value),

    /// Request that the VM deep-forces the value.
    DeepForceValue(Value),

    /// Request the value at the given index from the VM's with-stack, in forced
    /// state.
    ///
    /// The value is returned in the `ForceValue` message.
    WithValue(usize),

    /// Request the value at the given index from the *captured* with-stack, in
    /// forced state.
    CapturedWithValue(usize),

    /// Request that the two values be compared for Nix equality. The result is
    /// returned in the `ForceValue` message.
    NixEquality(Box<(Value, Value)>, PointerEquality),

    /// Push the given value to the VM's stack. This is used to prepare the
    /// stack for requesting a function call from the VM.
    ///
    /// The VM does not respond to this request, so the next message received is
    /// `Empty`.
    StackPush(Value),

    /// Pop a value from the stack and return it to the generator.
    StackPop,

    /// Request that the VM coerces this value to a string.
    StringCoerce(Value, CoercionKind),

    /// Request that the VM calls the given value, with arguments already
    /// prepared on the stack. Value must already be forced.
    Call(Value),

    /// Request a call frame entering the given lambda immediately. This can be
    /// used to force thunks.
    EnterLambda {
        lambda: Rc<Lambda>,
        upvalues: Rc<Upvalues>,
        span: Span,
    },

    /// Emit a runtime warning (already containing a span) through the VM.
    EmitWarning(EvalWarning),

    /// Emit a runtime warning through the VM. The span of the current generator
    /// is used for the final warning.
    EmitWarningKind(WarningKind),

    /// Request a lookup in the VM's import cache, which tracks the
    /// thunks yielded by previously imported files.
    ImportCacheLookup(PathBuf),

    /// Provide the VM with an imported value for a given path, which
    /// it can populate its input cache with.
    ImportCachePut(PathBuf, Value),

    /// Request that the VM imports the given path through its I/O interface.
    PathImport(PathBuf),

    /// Request that the VM opens the specified file and provides a reader.
    OpenFile(PathBuf),

    /// Request that the VM checks whether the given path exists.
    PathExists(PathBuf),

    /// Request that the VM reads the given path.
    ReadDir(PathBuf),

    /// Request a reasonable span from the VM.
    Span,

    /// Request evaluation of `builtins.tryEval` from the VM. See
    /// [`VM::catch_result`] for an explanation of how this works.
    TryForce(Value),

    /// Request serialisation of a value to JSON, according to the
    /// slightly odd Nix evaluation rules.
    ToJson(Value),
}

/// Human-readable representation of a generator message, used by observers.
impl Display for VMRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VMRequest::ForceValue(v) => write!(f, "force_value({})", v.type_of()),
            VMRequest::DeepForceValue(v) => {
                write!(f, "deep_force_value({})", v.type_of())
            }
            VMRequest::WithValue(_) => write!(f, "with_value"),
            VMRequest::CapturedWithValue(_) => write!(f, "captured_with_value"),
            VMRequest::NixEquality(values, ptr_eq) => {
                write!(
                    f,
                    "nix_eq({}, {}, PointerEquality::{:?})",
                    values.0.type_of(),
                    values.1.type_of(),
                    ptr_eq
                )
            }
            VMRequest::StackPush(v) => write!(f, "stack_push({})", v.type_of()),
            VMRequest::StackPop => write!(f, "stack_pop"),
            VMRequest::StringCoerce(
                v,
                CoercionKind {
                    strong,
                    import_paths,
                },
            ) => write!(
                f,
                "{}_{}importing_string_coerce({})",
                if *strong { "strong" } else { "weak" },
                if *import_paths { "" } else { "non_" },
                v.type_of()
            ),
            VMRequest::Call(v) => write!(f, "call({})", v),
            VMRequest::EnterLambda { lambda, .. } => {
                write!(f, "enter_lambda({:p})", *lambda)
            }
            VMRequest::EmitWarning(_) => write!(f, "emit_warning"),
            VMRequest::EmitWarningKind(_) => write!(f, "emit_warning_kind"),
            VMRequest::ImportCacheLookup(p) => {
                write!(f, "import_cache_lookup({})", p.to_string_lossy())
            }
            VMRequest::ImportCachePut(p, _) => {
                write!(f, "import_cache_put({})", p.to_string_lossy())
            }
            VMRequest::PathImport(p) => write!(f, "path_import({})", p.to_string_lossy()),
            VMRequest::OpenFile(p) => {
                write!(f, "open_file({})", p.to_string_lossy())
            }
            VMRequest::PathExists(p) => write!(f, "path_exists({})", p.to_string_lossy()),
            VMRequest::ReadDir(p) => write!(f, "read_dir({})", p.to_string_lossy()),
            VMRequest::Span => write!(f, "span"),
            VMRequest::TryForce(v) => write!(f, "try_force({})", v.type_of()),
            VMRequest::ToJson(v) => write!(f, "to_json({})", v.type_of()),
        }
    }
}

/// Responses returned to generators *from* the VM.
pub enum VMResponse {
    /// Empty message. Passed to the generator as the first message,
    /// or when return values were optional.
    Empty,

    /// Value produced by the VM and returned to the generator.
    Value(Value),

    /// Path produced by the VM in response to some IO operation.
    Path(PathBuf),

    /// VM response with the contents of a directory.
    Directory(Vec<(bytes::Bytes, FileType)>),

    /// VM response with a span to use at the current point.
    Span(Span),

    /// [std::io::Reader] produced by the VM in response to some IO operation.
    Reader(Box<dyn std::io::Read>),
}

impl Display for VMResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VMResponse::Empty => write!(f, "empty"),
            VMResponse::Value(v) => write!(f, "value({})", v),
            VMResponse::Path(p) => write!(f, "path({})", p.to_string_lossy()),
            VMResponse::Directory(d) => write!(f, "dir(len = {})", d.len()),
            VMResponse::Span(_) => write!(f, "span"),
            VMResponse::Reader(_) => write!(f, "reader"),
        }
    }
}

pub(crate) type Generator =
    Gen<VMRequest, VMResponse, Pin<Box<dyn Future<Output = Result<Value, ErrorKind>>>>>;

/// Helper function to provide type annotations which are otherwise difficult to
/// infer.
pub fn pin_generator(
    f: impl Future<Output = Result<Value, ErrorKind>> + 'static,
) -> Pin<Box<dyn Future<Output = Result<Value, ErrorKind>>>> {
    Box::pin(f)
}

impl<'o, IO> VM<'o, IO>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    /// Helper function to re-enqueue the current generator while it
    /// is awaiting a value.
    fn reenqueue_generator(&mut self, name: &'static str, span: Span, generator: Generator) {
        self.frames.push(Frame::Generator {
            name,
            generator,
            span,
            state: GeneratorState::AwaitingValue,
        });
    }

    /// Helper function to enqueue a new generator.
    pub(super) fn enqueue_generator<F, G>(&mut self, name: &'static str, span: Span, gen: G)
    where
        F: Future<Output = Result<Value, ErrorKind>> + 'static,
        G: FnOnce(GenCo) -> F,
    {
        self.frames.push(Frame::Generator {
            name,
            span,
            state: GeneratorState::Running,
            generator: Gen::new(|co| pin_generator(gen(co))),
        });
    }

    /// Run a generator frame until it yields to the outer control loop, or runs
    /// to completion.
    ///
    /// The return value indicates whether the generator has completed (true),
    /// or was suspended (false).
    pub(crate) fn run_generator(
        &mut self,
        name: &'static str,
        span: Span,
        frame_id: usize,
        state: GeneratorState,
        mut generator: Generator,
        initial_message: Option<VMResponse>,
    ) -> EvalResult<bool> {
        // Determine what to send to the generator based on its state.
        let mut message = match (initial_message, state) {
            (Some(msg), _) => msg,
            (_, GeneratorState::Running) => VMResponse::Empty,

            // If control returned here, and the generator is
            // awaiting a value, send it the top of the stack.
            (_, GeneratorState::AwaitingValue) => VMResponse::Value(self.stack_pop()),
        };

        loop {
            match generator.resume_with(message) {
                // If the generator yields, it contains an instruction
                // for what the VM should do.
                genawaiter::GeneratorState::Yielded(request) => {
                    self.observer.observe_generator_request(name, &request);

                    match request {
                        VMRequest::StackPush(value) => {
                            self.stack.push(value);
                            message = VMResponse::Empty;
                        }

                        VMRequest::StackPop => {
                            message = VMResponse::Value(self.stack_pop());
                        }

                        // Generator has requested a force, which means that
                        // this function prepares the frame stack and yields
                        // back to the outer VM loop.
                        VMRequest::ForceValue(value) => {
                            self.reenqueue_generator(name, span, generator);
                            self.enqueue_generator("force", span, |co| {
                                value.force_owned_genco(co, span)
                            });
                            return Ok(false);
                        }

                        // Generator has requested a deep-force.
                        VMRequest::DeepForceValue(value) => {
                            self.reenqueue_generator(name, span, generator);
                            self.enqueue_generator("deep_force", span, |co| {
                                value.deep_force(co, span)
                            });
                            return Ok(false);
                        }

                        // Generator has requested a value from the with-stack.
                        // Logic is similar to `ForceValue`, except with the
                        // value being taken from that stack.
                        VMRequest::WithValue(idx) => {
                            self.reenqueue_generator(name, span, generator);

                            let value = self.stack[self.with_stack[idx]].clone();
                            self.enqueue_generator("force", span, |co| {
                                value.force_owned_genco(co, span)
                            });

                            return Ok(false);
                        }

                        // Generator has requested a value from the *captured*
                        // with-stack. Logic is same as above, except for the
                        // value being from that stack.
                        VMRequest::CapturedWithValue(idx) => {
                            self.reenqueue_generator(name, span, generator);

                            let call_frame = self.last_call_frame()
                                .expect("Tvix bug: generator requested captured with-value, but there is no call frame");

                            let value = call_frame.upvalues.with_stack().unwrap()[idx].clone();
                            self.enqueue_generator("force", span, |co| {
                                value.force_owned_genco(co, span)
                            });

                            return Ok(false);
                        }

                        VMRequest::NixEquality(values, ptr_eq) => {
                            let values = *values;
                            self.reenqueue_generator(name, span, generator);
                            self.enqueue_generator("nix_eq", span, |co| {
                                values.0.nix_eq_owned_genco(values.1, co, ptr_eq, span)
                            });
                            return Ok(false);
                        }

                        VMRequest::StringCoerce(val, kind) => {
                            self.reenqueue_generator(name, span, generator);
                            self.enqueue_generator("coerce_to_string", span, |co| {
                                val.coerce_to_string(co, kind, span)
                            });
                            return Ok(false);
                        }

                        VMRequest::Call(callable) => {
                            self.reenqueue_generator(name, span, generator);
                            self.call_value(span, None, callable)?;
                            return Ok(false);
                        }

                        VMRequest::EnterLambda {
                            lambda,
                            upvalues,
                            span,
                        } => {
                            self.reenqueue_generator(name, span, generator);

                            self.frames.push(Frame::CallFrame {
                                span,
                                call_frame: CallFrame {
                                    lambda,
                                    upvalues,
                                    ip: CodeIdx(0),
                                    stack_offset: self.stack.len(),
                                },
                            });

                            return Ok(false);
                        }

                        VMRequest::EmitWarning(warning) => {
                            self.push_warning(warning);
                            message = VMResponse::Empty;
                        }

                        VMRequest::EmitWarningKind(kind) => {
                            self.emit_warning(kind);
                            message = VMResponse::Empty;
                        }

                        VMRequest::ImportCacheLookup(path) => {
                            if let Some(cached) = self.import_cache.get(path) {
                                message = VMResponse::Value(cached.clone());
                            } else {
                                message = VMResponse::Empty;
                            }
                        }

                        VMRequest::ImportCachePut(path, value) => {
                            self.import_cache.insert(path, value);
                            message = VMResponse::Empty;
                        }

                        VMRequest::PathImport(path) => {
                            let imported = self
                                .io_handle
                                .as_ref()
                                .import_path(&path)
                                .map_err(|e| ErrorKind::IO {
                                    path: Some(path),
                                    error: e.into(),
                                })
                                .with_span(span, self)?;

                            message = VMResponse::Path(imported);
                        }

                        VMRequest::OpenFile(path) => {
                            let reader = self
                                .io_handle
                                .as_ref()
                                .open(&path)
                                .map_err(|e| ErrorKind::IO {
                                    path: Some(path),
                                    error: e.into(),
                                })
                                .with_span(span, self)?;

                            message = VMResponse::Reader(reader)
                        }

                        VMRequest::PathExists(path) => {
                            let exists = self
                                .io_handle
                                .as_ref()
                                .path_exists(&path)
                                .map_err(|e| ErrorKind::IO {
                                    path: Some(path),
                                    error: e.into(),
                                })
                                .map(Value::Bool)
                                .with_span(span, self)?;

                            message = VMResponse::Value(exists);
                        }

                        VMRequest::ReadDir(path) => {
                            let dir = self
                                .io_handle
                                .as_ref()
                                .read_dir(&path)
                                .map_err(|e| ErrorKind::IO {
                                    path: Some(path),
                                    error: e.into(),
                                })
                                .with_span(span, self)?;
                            message = VMResponse::Directory(dir);
                        }

                        VMRequest::Span => {
                            message = VMResponse::Span(self.reasonable_span);
                        }

                        VMRequest::TryForce(value) => {
                            self.try_eval_frames.push(frame_id);
                            self.reenqueue_generator(name, span, generator);

                            debug_assert!(
                                self.frames.len() == frame_id + 1,
                                "generator should be reenqueued with the same frame ID"
                            );

                            self.enqueue_generator("force", span, |co| {
                                value.force_owned_genco(co, span)
                            });
                            return Ok(false);
                        }

                        VMRequest::ToJson(value) => {
                            self.reenqueue_generator(name, span, generator);
                            self.enqueue_generator("to_json", span, |co| {
                                value.into_contextful_json_generator(co)
                            });
                            return Ok(false);
                        }
                    }
                }

                // Generator has completed, and its result value should
                // be left on the stack.
                genawaiter::GeneratorState::Complete(result) => {
                    let value = result.with_span(span, self)?;
                    self.stack.push(value);
                    return Ok(true);
                }
            }
        }
    }
}

pub type GenCo = Co<VMRequest, VMResponse>;

// -- Implementation of concrete generator use-cases.

/// Request that the VM place the given value on its stack.
pub async fn request_stack_push(co: &GenCo, val: Value) {
    match co.yield_(VMRequest::StackPush(val)).await {
        VMResponse::Empty => {}
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM pop a value from the stack and return it to the
/// generator.
pub async fn request_stack_pop(co: &GenCo) -> Value {
    match co.yield_(VMRequest::StackPop).await {
        VMResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Force any value and return the evaluated result from the VM.
pub async fn request_force(co: &GenCo, val: Value) -> Value {
    if let Value::Thunk(_) = val {
        match co.yield_(VMRequest::ForceValue(val)).await {
            VMResponse::Value(value) => value,
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        }
    } else {
        val
    }
}

/// Force a value
pub(crate) async fn request_try_force(co: &GenCo, val: Value) -> Value {
    if let Value::Thunk(_) = val {
        match co.yield_(VMRequest::TryForce(val)).await {
            VMResponse::Value(value) => value,
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        }
    } else {
        val
    }
}

/// Call the given value as a callable. The argument(s) must already be prepared
/// on the stack.
pub async fn request_call(co: &GenCo, val: Value) -> Value {
    let val = request_force(co, val).await;
    match co.yield_(VMRequest::Call(val)).await {
        VMResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Helper function to call the given value with the provided list of arguments.
/// This uses the StackPush and Call messages under the hood.
pub async fn request_call_with<I>(co: &GenCo, mut callable: Value, args: I) -> Value
where
    I: IntoIterator<Item = Value>,
    I::IntoIter: DoubleEndedIterator,
{
    let mut num_args = 0_usize;
    for arg in args.into_iter().rev() {
        num_args += 1;
        request_stack_push(co, arg).await;
    }

    debug_assert!(num_args > 0, "call_with called with an empty list of args");

    while num_args > 0 {
        callable = request_call(co, callable).await;
        num_args -= 1;
    }

    callable
}

pub async fn request_string_coerce(
    co: &GenCo,
    val: Value,
    kind: CoercionKind,
) -> Result<NixString, CatchableErrorKind> {
    match val {
        Value::String(s) => Ok(s),
        _ => match co.yield_(VMRequest::StringCoerce(val, kind)).await {
            VMResponse::Value(Value::Catchable(c)) => Err(*c),
            VMResponse::Value(value) => Ok(value
                .to_contextful_str()
                .expect("coerce_to_string always returns a string")),
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        },
    }
}

/// Deep-force any value and return the evaluated result from the VM.
pub async fn request_deep_force(co: &GenCo, val: Value) -> Value {
    match co.yield_(VMRequest::DeepForceValue(val)).await {
        VMResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Ask the VM to compare two values for equality.
pub(crate) async fn check_equality(
    co: &GenCo,
    a: Value,
    b: Value,
    ptr_eq: PointerEquality,
) -> Result<Result<bool, CatchableErrorKind>, ErrorKind> {
    match co
        .yield_(VMRequest::NixEquality(Box::new((a, b)), ptr_eq))
        .await
    {
        VMResponse::Value(Value::Bool(b)) => Ok(Ok(b)),
        VMResponse::Value(Value::Catchable(cek)) => Ok(Err(*cek)),
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Emit a fully constructed runtime warning.
pub(crate) async fn emit_warning(co: &GenCo, warning: EvalWarning) {
    match co.yield_(VMRequest::EmitWarning(warning)).await {
        VMResponse::Empty => {}
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Emit a runtime warning with the span of the current generator.
pub async fn emit_warning_kind(co: &GenCo, kind: WarningKind) {
    match co.yield_(VMRequest::EmitWarningKind(kind)).await {
        VMResponse::Empty => {}
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM enter the given lambda.
pub(crate) async fn request_enter_lambda(
    co: &GenCo,
    lambda: Rc<Lambda>,
    upvalues: Rc<Upvalues>,
    span: Span,
) -> Value {
    let msg = VMRequest::EnterLambda {
        lambda,
        upvalues,
        span,
    };

    match co.yield_(msg).await {
        VMResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request a lookup in the VM's import cache.
pub(crate) async fn request_import_cache_lookup(co: &GenCo, path: PathBuf) -> Option<Value> {
    match co.yield_(VMRequest::ImportCacheLookup(path)).await {
        VMResponse::Value(value) => Some(value),
        VMResponse::Empty => None,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM populate its input cache for the given path.
pub(crate) async fn request_import_cache_put(co: &GenCo, path: PathBuf, value: Value) {
    match co.yield_(VMRequest::ImportCachePut(path, value)).await {
        VMResponse::Empty => {}
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM import the given path.
pub(crate) async fn request_path_import(co: &GenCo, path: PathBuf) -> PathBuf {
    match co.yield_(VMRequest::PathImport(path)).await {
        VMResponse::Path(path) => path,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM open a [std::io::Read] for the specified file.
pub async fn request_open_file(co: &GenCo, path: PathBuf) -> Box<dyn std::io::Read> {
    match co.yield_(VMRequest::OpenFile(path)).await {
        VMResponse::Reader(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

#[cfg_attr(not(feature = "impure"), allow(unused))]
pub(crate) async fn request_path_exists(co: &GenCo, path: PathBuf) -> Value {
    match co.yield_(VMRequest::PathExists(path)).await {
        VMResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

#[cfg_attr(not(feature = "impure"), allow(unused))]
pub(crate) async fn request_read_dir(co: &GenCo, path: PathBuf) -> Vec<(bytes::Bytes, FileType)> {
    match co.yield_(VMRequest::ReadDir(path)).await {
        VMResponse::Directory(dir) => dir,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

pub(crate) async fn request_span(co: &GenCo) -> Span {
    match co.yield_(VMRequest::Span).await {
        VMResponse::Span(span) => span,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

pub(crate) async fn request_to_json(
    co: &GenCo,
    value: Value,
) -> Result<(serde_json::Value, NixContext), CatchableErrorKind> {
    match co.yield_(VMRequest::ToJson(value)).await {
        VMResponse::Value(Value::Json(json_with_ctx)) => Ok(*json_with_ctx),
        VMResponse::Value(Value::Catchable(cek)) => Err(*cek),
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Call the given value as if it was an attribute set containing a functor. The
/// arguments must already be prepared on the stack when a generator frame from
/// this function is invoked.
///
pub(crate) async fn call_functor(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
    let attrs = value.to_attrs()?;

    match attrs.select("__functor") {
        None => Err(ErrorKind::NotCallable("set without `__functor_` attribute")),
        Some(functor) => {
            // The functor receives the set itself as its first argument and
            // needs to be called with it.
            let functor = request_force(&co, functor.clone()).await;
            let primed = request_call_with(&co, functor, [value]).await;
            Ok(request_call(&co, primed).await)
        }
    }
}
