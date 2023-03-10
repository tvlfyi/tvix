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
use smol_str::SmolStr;
use std::fmt::Display;
use std::future::Future;

use crate::value::{PointerEquality, SharedThunkSet};
use crate::warnings::WarningKind;
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

/// Messages that can be sent from generators to the VM. In most
/// cases, the VM will suspend the generator when receiving a message
/// and enter some other frame to process the request.
///
/// Responses are returned to generators via the [`GeneratorResponse`] type.
pub enum GeneratorRequest {
    /// Request that the VM forces this value. This message is first sent to the
    /// VM with the unforced value, then returned to the generator with the
    /// forced result.
    ForceValue(Value),

    /// Request that the VM deep-forces the value.
    DeepForceValue(Value, SharedThunkSet),

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
    /// a `NoOp`.
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
        light_span: LightSpan,
    },

    /// Emit a runtime warning through the VM. Receives a NoOp-response.
    EmitWarning(WarningKind),

    /// Request a lookup in the VM's import cache, which tracks the
    /// thunks yielded by previously imported files.
    ImportCacheLookup(PathBuf),

    /// Provide the VM with an imported value for a given path, which
    /// it can populate its input cache with.
    ImportCachePut(PathBuf, Value),

    /// Request that the VM imports the given path through its I/O interface.
    PathImport(PathBuf),

    /// Request that the VM reads the given path to a string.
    ReadToString(PathBuf),

    /// Request that the VM checks whether the given path exists.
    PathExists(PathBuf),

    /// Request that the VM reads the given path.
    ReadDir(PathBuf),

    /// Request a reasonable span from the VM.
    Span,

    /// Request evaluation of `builtins.tryEval` from the VM. See
    /// [`VM::catch_result`] for an explanation of how this works.
    TryForce(Value),
}

/// Human-readable representation of a generator message, used by observers.
impl Display for GeneratorRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeneratorRequest::ForceValue(v) => write!(f, "force_value({})", v),
            GeneratorRequest::DeepForceValue(v, _) => write!(f, "deep_force_value({})", v),
            GeneratorRequest::WithValue(_) => write!(f, "with_value"),
            GeneratorRequest::CapturedWithValue(_) => write!(f, "captured_with_value"),
            GeneratorRequest::NixEquality(values, ptr_eq) => {
                write!(
                    f,
                    "nix_eq({}, {}, PointerEquality::{:?})",
                    values.0, values.1, ptr_eq
                )
            }
            GeneratorRequest::StackPush(v) => write!(f, "stack_push({})", v),
            GeneratorRequest::StackPop => write!(f, "stack_pop"),
            GeneratorRequest::StringCoerce(v, kind) => match kind {
                CoercionKind::Weak => write!(f, "weak_string_coerce({})", v),
                CoercionKind::Strong => write!(f, "strong_string_coerce({})", v),
                CoercionKind::ThunksOnly => todo!("remove this branch (not live)"),
            },
            GeneratorRequest::Call(v) => write!(f, "call({})", v),
            GeneratorRequest::EnterLambda { lambda, .. } => {
                write!(f, "enter_lambda({:p})", *lambda)
            }
            GeneratorRequest::EmitWarning(_) => write!(f, "emit_warning"),
            GeneratorRequest::ImportCacheLookup(p) => {
                write!(f, "import_cache_lookup({})", p.to_string_lossy())
            }
            GeneratorRequest::ImportCachePut(p, _) => {
                write!(f, "import_cache_put({})", p.to_string_lossy())
            }
            GeneratorRequest::PathImport(p) => write!(f, "path_import({})", p.to_string_lossy()),
            GeneratorRequest::ReadToString(p) => {
                write!(f, "read_to_string({})", p.to_string_lossy())
            }
            GeneratorRequest::PathExists(p) => write!(f, "path_exists({})", p.to_string_lossy()),
            GeneratorRequest::ReadDir(p) => write!(f, "read_dir({})", p.to_string_lossy()),
            GeneratorRequest::Span => write!(f, "span"),
            GeneratorRequest::TryForce(v) => write!(f, "try_force({})", v),
        }
    }
}

/// Responses returned to generators from the VM.
pub enum GeneratorResponse {
    /// Empty message. Passed to the generator as the first message,
    /// or when return values were optional.
    Empty,

    /// Value produced by the VM and returned to the generator.
    Value(Value),

    /// Path produced by the VM in response to some IO operation.
    Path(PathBuf),

    /// VM response with the contents of a directory.
    Directory(Vec<(SmolStr, FileType)>),

    /// VM response with a span to use at the current point.
    Span(LightSpan),

    /// Message returned by the VM when a catchable error is encountered during
    /// the evaluation of `builtins.tryEval`.
    ForceError,
}

impl Display for GeneratorResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeneratorResponse::Empty => write!(f, "empty"),
            GeneratorResponse::Value(v) => write!(f, "value({})", v),
            GeneratorResponse::Path(p) => write!(f, "path({})", p.to_string_lossy()),
            GeneratorResponse::Directory(d) => write!(f, "dir(len = {})", d.len()),
            GeneratorResponse::Span(_) => write!(f, "span"),
            GeneratorResponse::ForceError => write!(f, "force_error"),
        }
    }
}

pub(crate) type Generator = Gen<
    GeneratorRequest,
    GeneratorResponse,
    Pin<Box<dyn Future<Output = Result<Value, ErrorKind>>>>,
>;

/// Helper function to provide type annotations which are otherwise difficult to
/// infer.
pub fn pin_generator(
    f: impl Future<Output = Result<Value, ErrorKind>> + 'static,
) -> Pin<Box<dyn Future<Output = Result<Value, ErrorKind>>>> {
    Box::pin(f)
}

pub type GenCo = Co<GeneratorRequest, GeneratorResponse>;

// -- Implementation of concrete generator use-cases.

/// Request that the VM place the given value on its stack.
pub async fn request_stack_push(co: &GenCo, val: Value) {
    match co.yield_(GeneratorRequest::StackPush(val)).await {
        GeneratorResponse::Empty => {}
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM pop a value from the stack and return it to the
/// generator.
pub async fn request_stack_pop(co: &GenCo) -> Value {
    match co.yield_(GeneratorRequest::StackPop).await {
        GeneratorResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Force any value and return the evaluated result from the VM.
pub async fn request_force(co: &GenCo, val: Value) -> Value {
    if let Value::Thunk(_) = val {
        match co.yield_(GeneratorRequest::ForceValue(val)).await {
            GeneratorResponse::Value(value) => value,
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        }
    } else {
        val
    }
}

/// Force a value, but inform the caller (by returning `None`) if a catchable
/// error occured.
pub(crate) async fn request_try_force(co: &GenCo, val: Value) -> Option<Value> {
    if let Value::Thunk(_) = val {
        match co.yield_(GeneratorRequest::TryForce(val)).await {
            GeneratorResponse::Value(value) => Some(value),
            GeneratorResponse::ForceError => None,
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        }
    } else {
        Some(val)
    }
}

/// Call the given value as a callable. The argument(s) must already be prepared
/// on the stack.
pub async fn request_call(co: &GenCo, val: Value) -> Value {
    let val = request_force(co, val).await;
    match co.yield_(GeneratorRequest::Call(val)).await {
        GeneratorResponse::Value(value) => value,
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

pub async fn request_string_coerce(co: &GenCo, val: Value, kind: CoercionKind) -> NixString {
    match val {
        Value::String(s) => s,
        _ => match co.yield_(GeneratorRequest::StringCoerce(val, kind)).await {
            GeneratorResponse::Value(value) => value
                .to_str()
                .expect("coerce_to_string always returns a string"),
            msg => panic!(
                "Tvix bug: VM responded with incorrect generator message: {}",
                msg
            ),
        },
    }
}

/// Deep-force any value and return the evaluated result from the VM.
pub async fn request_deep_force(co: &GenCo, val: Value, thunk_set: SharedThunkSet) -> Value {
    match co
        .yield_(GeneratorRequest::DeepForceValue(val, thunk_set))
        .await
    {
        GeneratorResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Fetch and force a value on the with-stack from the VM.
async fn fetch_forced_with(co: &GenCo, idx: usize) -> Value {
    match co.yield_(GeneratorRequest::WithValue(idx)).await {
        GeneratorResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Fetch and force a value on the *captured* with-stack from the VM.
async fn fetch_captured_with(co: &GenCo, idx: usize) -> Value {
    match co.yield_(GeneratorRequest::CapturedWithValue(idx)).await {
        GeneratorResponse::Value(value) => value,
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
) -> Result<bool, ErrorKind> {
    match co
        .yield_(GeneratorRequest::NixEquality(Box::new((a, b)), ptr_eq))
        .await
    {
        GeneratorResponse::Value(value) => value.as_bool(),
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Emit a runtime warning.
pub(crate) async fn emit_warning(co: &GenCo, kind: WarningKind) {
    match co.yield_(GeneratorRequest::EmitWarning(kind)).await {
        GeneratorResponse::Empty => {}
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
    light_span: LightSpan,
) -> Value {
    let msg = GeneratorRequest::EnterLambda {
        lambda,
        upvalues,
        light_span,
    };

    match co.yield_(msg).await {
        GeneratorResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request a lookup in the VM's import cache.
pub(crate) async fn request_import_cache_lookup(co: &GenCo, path: PathBuf) -> Option<Value> {
    match co.yield_(GeneratorRequest::ImportCacheLookup(path)).await {
        GeneratorResponse::Value(value) => Some(value),
        GeneratorResponse::Empty => None,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM populate its input cache for the given path.
pub(crate) async fn request_import_cache_put(co: &GenCo, path: PathBuf, value: Value) {
    match co
        .yield_(GeneratorRequest::ImportCachePut(path, value))
        .await
    {
        GeneratorResponse::Empty => {}
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

/// Request that the VM import the given path.
pub(crate) async fn request_path_import(co: &GenCo, path: PathBuf) -> PathBuf {
    match co.yield_(GeneratorRequest::PathImport(path)).await {
        GeneratorResponse::Path(path) => path,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

pub(crate) async fn request_read_to_string(co: &GenCo, path: PathBuf) -> Value {
    match co.yield_(GeneratorRequest::ReadToString(path)).await {
        GeneratorResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

pub(crate) async fn request_path_exists(co: &GenCo, path: PathBuf) -> Value {
    match co.yield_(GeneratorRequest::PathExists(path)).await {
        GeneratorResponse::Value(value) => value,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

pub(crate) async fn request_read_dir(co: &GenCo, path: PathBuf) -> Vec<(SmolStr, FileType)> {
    match co.yield_(GeneratorRequest::ReadDir(path)).await {
        GeneratorResponse::Directory(dir) => dir,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

pub(crate) async fn request_span(co: &GenCo) -> LightSpan {
    match co.yield_(GeneratorRequest::Span).await {
        GeneratorResponse::Span(span) => span,
        msg => panic!(
            "Tvix bug: VM responded with incorrect generator message: {}",
            msg
        ),
    }
}

pub(crate) async fn neo_resolve_with(
    co: GenCo,
    ident: String,
    vm_with_len: usize,
    upvalue_with_len: usize,
) -> Result<Value, ErrorKind> {
    for with_stack_idx in (0..vm_with_len).rev() {
        // TODO(tazjin): is this branch still live with the current with-thunking?
        let with = fetch_forced_with(&co, with_stack_idx).await;

        match with.to_attrs()?.select(&ident) {
            None => continue,
            Some(val) => return Ok(val.clone()),
        }
    }

    for upvalue_with_idx in (0..upvalue_with_len).rev() {
        let with = fetch_captured_with(&co, upvalue_with_idx).await;

        match with.to_attrs()?.select(&ident) {
            None => continue,
            Some(val) => return Ok(val.clone()),
        }
    }

    Err(ErrorKind::UnknownDynamicVariable(ident))
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
