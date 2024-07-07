//! `tvix-eval` implements the evaluation of the Nix programming language in
//! Tvix.
//!
//! It is designed to allow users to use Nix as a versatile language for
//! different use-cases.
//!
//! This module exports the high-level functions and types needed for evaluating
//! Nix code and interacting with the language's data structures.
//!
//! Nix has several language features that make use of impurities (such as
//! reading from the NIX_PATH environment variable, or interacting with files).
//! These features are optional and the API of this crate exposes functionality
//! for controlling how they work.

pub mod builtins;
mod chunk;
mod compiler;
mod errors;
mod io;
pub mod observer;
mod opcode;
mod pretty_ast;
mod source;
mod spans;
mod systems;
mod upvalues;
mod value;
mod vm;
mod warnings;

mod nix_search_path;
#[cfg(all(test, feature = "arbitrary"))]
mod properties;
#[cfg(test)]
mod test_utils;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use crate::observer::{CompilerObserver, RuntimeObserver};
use crate::value::Lambda;
use crate::vm::run_lambda;

// Re-export the public interface used by other crates.
pub use crate::compiler::{compile, prepare_globals, CompilationOutput, GlobalsMap};
pub use crate::errors::{AddContext, CatchableErrorKind, Error, ErrorKind, EvalResult};
pub use crate::io::{DummyIO, EvalIO, FileType};
pub use crate::pretty_ast::pretty_print_expr;
pub use crate::source::SourceCode;
pub use crate::value::{NixContext, NixContextElement};
pub use crate::vm::generators;
pub use crate::warnings::{EvalWarning, WarningKind};
pub use builtin_macros;
use smol_str::SmolStr;

pub use crate::value::{Builtin, CoercionKind, NixAttrs, NixList, NixString, Value};

#[cfg(feature = "impure")]
pub use crate::io::StdIO;

struct BuilderBuiltins {
    builtins: Vec<(&'static str, Value)>,
    src_builtins: Vec<(&'static str, &'static str)>,
}

enum BuilderGlobals {
    Builtins(BuilderBuiltins),
    Globals(Rc<GlobalsMap>),
}

/// Builder for building an [`Evaluation`].
///
/// Construct an [`EvaluationBuilder`] by calling one of:
///
/// - [`Evaluation::builder`] / [`EvaluationBuilder::new`]
/// - [`Evaluation::builder_impure`] [`EvaluationBuilder::new_impure`]
/// - [`Evaluation::builder_pure`] [`EvaluationBuilder::new_pure`]
///
/// Then configure the fields by calling the various methods on [`EvaluationBuilder`], and finally
/// call [`build`](Self::build) to construct an [`Evaluation`]
pub struct EvaluationBuilder<'co, 'ro, 'env, IO> {
    source_map: Option<SourceCode>,
    globals: BuilderGlobals,
    env: Option<&'env HashMap<SmolStr, Value>>,
    io_handle: IO,
    enable_import: bool,
    strict: bool,
    nix_path: Option<String>,
    compiler_observer: Option<&'co mut dyn CompilerObserver>,
    runtime_observer: Option<&'ro mut dyn RuntimeObserver>,
}

impl<'co, 'ro, 'env, IO> EvaluationBuilder<'co, 'ro, 'env, IO>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    /// Build an [`Evaluation`] based on the configuration in this builder.
    ///
    /// This:
    ///
    /// - Adds a `"storeDir"` builtin containing the store directory of the configured IO handle
    /// - Sets up globals based on the configured builtins
    /// - Copies all other configured fields to the [`Evaluation`]
    pub fn build(self) -> Evaluation<'co, 'ro, 'env, IO> {
        let source_map = self.source_map.unwrap_or_default();

        let globals = match self.globals {
            BuilderGlobals::Globals(globals) => globals,
            BuilderGlobals::Builtins(BuilderBuiltins {
                mut builtins,
                src_builtins,
            }) => {
                // Insert a storeDir builtin *iff* a store directory is present.
                if let Some(store_dir) = self.io_handle.as_ref().store_dir() {
                    builtins.push(("storeDir", store_dir.into()));
                }

                crate::compiler::prepare_globals(
                    builtins,
                    src_builtins,
                    source_map.clone(),
                    self.enable_import,
                )
            }
        };

        Evaluation {
            source_map,
            globals,
            env: self.env,
            io_handle: self.io_handle,
            strict: self.strict,
            nix_path: self.nix_path,
            compiler_observer: self.compiler_observer,
            runtime_observer: self.runtime_observer,
        }
    }
}

// NOTE(aspen): The methods here are intentionally incomplete; feel free to add new ones (ideally
// with similar naming conventions to the ones already present) but don't expose fields publically!
impl<'co, 'ro, 'env, IO> EvaluationBuilder<'co, 'ro, 'env, IO> {
    pub fn new(io_handle: IO) -> Self {
        let mut builtins = builtins::pure_builtins();
        builtins.extend(builtins::placeholders()); // these are temporary

        Self {
            source_map: None,
            enable_import: false,
            io_handle,
            globals: BuilderGlobals::Builtins(BuilderBuiltins {
                builtins,
                src_builtins: vec![],
            }),
            env: None,
            strict: false,
            nix_path: None,
            compiler_observer: None,
            runtime_observer: None,
        }
    }

    pub fn io_handle<IO2>(self, io_handle: IO2) -> EvaluationBuilder<'co, 'ro, 'env, IO2> {
        EvaluationBuilder {
            io_handle,
            source_map: self.source_map,
            globals: self.globals,
            env: self.env,
            enable_import: self.enable_import,
            strict: self.strict,
            nix_path: self.nix_path,
            compiler_observer: self.compiler_observer,
            runtime_observer: self.runtime_observer,
        }
    }

    pub fn with_enable_import(self, enable_import: bool) -> Self {
        Self {
            enable_import,
            ..self
        }
    }

    pub fn disable_import(self) -> Self {
        self.with_enable_import(false)
    }

    pub fn enable_import(self) -> Self {
        self.with_enable_import(true)
    }

    fn builtins_mut(&mut self) -> &mut BuilderBuiltins {
        match &mut self.globals {
            BuilderGlobals::Builtins(builtins) => builtins,
            BuilderGlobals::Globals(_) => {
                panic!("Cannot modify builtins on an EvaluationBuilder with globals configured")
            }
        }
    }

    /// Add additional builtins (represented as tuples of name and [`Value`]) to this evaluation
    /// builder.
    ///
    /// # Panics
    ///
    /// Panics if this evaluation builder has had globals set via [`with_globals`]
    pub fn add_builtins<I>(mut self, builtins: I) -> Self
    where
        I: IntoIterator<Item = (&'static str, Value)>,
    {
        self.builtins_mut().builtins.extend(builtins);
        self
    }

    /// Add additional builtins that are implemented in Nix source code (represented as tuples of
    /// name and nix source) to this evaluation builder.
    ///
    /// # Panics
    ///
    /// Panics if this evaluation builder has had globals set via [`with_globals`]
    pub fn add_src_builtin(mut self, name: &'static str, src: &'static str) -> Self {
        self.builtins_mut().src_builtins.push((name, src));
        self
    }

    /// Set the globals for this evaluation builder to a previously-constructed globals map.
    /// Intended to allow sharing globals across multiple evaluations (eg for the REPL).
    ///
    /// Discards any builtins previously configured via [`add_builtins`] and [`add_src_builtins`].
    /// If either of those methods is called on the evaluation builder after this one, they will
    /// panic.
    pub fn with_globals(self, globals: Rc<GlobalsMap>) -> Self {
        Self {
            globals: BuilderGlobals::Globals(globals),
            ..self
        }
    }

    pub fn with_source_map(self, source_map: SourceCode) -> Self {
        debug_assert!(
            self.source_map.is_none(),
            "Cannot set the source_map on an EvaluationBuilder twice"
        );
        Self {
            source_map: Some(source_map),
            ..self
        }
    }

    pub fn with_strict(self, strict: bool) -> Self {
        Self { strict, ..self }
    }

    pub fn strict(self) -> Self {
        self.with_strict(true)
    }

    pub fn nix_path(self, nix_path: Option<String>) -> Self {
        Self { nix_path, ..self }
    }

    pub fn env(self, env: Option<&'env HashMap<SmolStr, Value>>) -> Self {
        Self { env, ..self }
    }

    pub fn compiler_observer(
        self,
        compiler_observer: Option<&'co mut dyn CompilerObserver>,
    ) -> Self {
        Self {
            compiler_observer,
            ..self
        }
    }

    pub fn set_compiler_observer(
        &mut self,
        compiler_observer: Option<&'co mut dyn CompilerObserver>,
    ) {
        self.compiler_observer = compiler_observer;
    }

    pub fn runtime_observer(self, runtime_observer: Option<&'ro mut dyn RuntimeObserver>) -> Self {
        Self {
            runtime_observer,
            ..self
        }
    }

    pub fn set_runtime_observer(&mut self, runtime_observer: Option<&'ro mut dyn RuntimeObserver>) {
        self.runtime_observer = runtime_observer;
    }
}

impl<'co, 'ro, 'env, IO> EvaluationBuilder<'co, 'ro, 'env, IO> {
    pub fn source_map(&mut self) -> &SourceCode {
        self.source_map.get_or_insert_with(SourceCode::default)
    }
}

impl<'co, 'ro, 'env> EvaluationBuilder<'co, 'ro, 'env, Box<dyn EvalIO>> {
    /// Initialize an `Evaluation`, without the import statement available, and
    /// all IO operations stubbed out.
    pub fn new_pure() -> Self {
        Self::new(Box::new(DummyIO) as Box<dyn EvalIO>).with_enable_import(false)
    }

    #[cfg(feature = "impure")]
    /// Configure an `Evaluation` to have impure features available
    /// with the given I/O implementation.
    ///
    /// If no I/O implementation is supplied, [`StdIO`] is used by
    /// default.
    pub fn enable_impure(mut self, io: Option<Box<dyn EvalIO>>) -> Self {
        self.io_handle = io.unwrap_or_else(|| Box::new(StdIO) as Box<dyn EvalIO>);
        self.enable_import = true;
        self.builtins_mut()
            .builtins
            .extend(builtins::impure_builtins());

        // Make `NIX_PATH` resolutions work by default, unless the
        // user already overrode this with something else.
        if self.nix_path.is_none() {
            self.nix_path = std::env::var("NIX_PATH").ok();
        }
        self
    }

    #[cfg(feature = "impure")]
    /// Initialise an `Evaluation`, with all impure features turned on by default.
    pub fn new_impure() -> Self {
        Self::new_pure().enable_impure(None)
    }
}

/// An `Evaluation` represents how a piece of Nix code is evaluated. It can be
/// instantiated and configured directly, or it can be accessed through the
/// various simplified helper methods available below.
///
/// Public fields are intended to be set by the caller. Setting all
/// fields is optional.
pub struct Evaluation<'co, 'ro, 'env, IO> {
    /// Source code map used for error reporting.
    source_map: SourceCode,

    /// Set of all global values available at the top-level scope
    globals: Rc<GlobalsMap>,

    /// Top-level variables to define in the evaluation
    env: Option<&'env HashMap<SmolStr, Value>>,

    /// Implementation of file-IO to use during evaluation, e.g. for
    /// impure builtins.
    ///
    /// Defaults to [`DummyIO`] if not set explicitly.
    io_handle: IO,

    /// Determines whether the returned value should be strictly
    /// evaluated, that is whether its list and attribute set elements
    /// should be forced recursively.
    strict: bool,

    /// (optional) Nix search path, e.g. the value of `NIX_PATH` used
    /// for resolving items on the search path (such as `<nixpkgs>`).
    nix_path: Option<String>,

    /// (optional) compiler observer for reporting on compilation
    /// details, like the emitted bytecode.
    compiler_observer: Option<&'co mut dyn CompilerObserver>,

    /// (optional) runtime observer, for reporting on execution steps
    /// of Nix code.
    runtime_observer: Option<&'ro mut dyn RuntimeObserver>,
}

/// Result of evaluating a piece of Nix code. If evaluation succeeded, a value
/// will be present (and potentially some warnings!). If evaluation failed,
/// errors will be present.
#[derive(Debug, Default)]
pub struct EvaluationResult {
    /// Nix value that the code evaluated to.
    pub value: Option<Value>,

    /// Errors that occured during evaluation (if any).
    pub errors: Vec<Error>,

    /// Warnings that occured during evaluation. Warnings are not critical, but
    /// should be addressed either to modernise code or improve performance.
    pub warnings: Vec<EvalWarning>,

    /// AST node that was parsed from the code (on success only).
    pub expr: Option<rnix::ast::Expr>,
}

impl<'co, 'ro, 'env, IO> Evaluation<'co, 'ro, 'env, IO> {
    /// Make a new [builder][] for configuring an evaluation
    ///
    /// [builder]: EvaluationBuilder
    pub fn builder(io_handle: IO) -> EvaluationBuilder<'co, 'ro, 'env, IO> {
        EvaluationBuilder::new(io_handle)
    }

    /// Clone the reference to the map of Nix globals for this evaluation. If [`Value`]s are shared
    /// across subsequent [`Evaluation`]s, it is important that those evaluations all have the same
    /// underlying globals map.
    pub fn globals(&self) -> Rc<GlobalsMap> {
        self.globals.clone()
    }

    /// Clone the reference to the contained source code map. This is used after an evaluation for
    /// pretty error printing. Also, if [`Value`]s are shared across subsequent [`Evaluation`]s, it
    /// is important that those evaluations all have the same underlying source code map.
    pub fn source_map(&self) -> SourceCode {
        self.source_map.clone()
    }
}

impl<'co, 'ro, 'env> Evaluation<'co, 'ro, 'env, Box<dyn EvalIO>> {
    #[cfg(feature = "impure")]
    pub fn builder_impure() -> EvaluationBuilder<'co, 'ro, 'env, Box<dyn EvalIO>> {
        EvaluationBuilder::new_impure()
    }

    pub fn builder_pure() -> EvaluationBuilder<'co, 'ro, 'env, Box<dyn EvalIO>> {
        EvaluationBuilder::new_pure()
    }
}

impl<'co, 'ro, 'env, IO> Evaluation<'co, 'ro, 'env, IO>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    /// Only compile the provided source code, at an optional location of the
    /// source code (i.e. path to the file it was read from; used for error
    /// reporting, and for resolving relative paths in impure functions)
    /// This does not *run* the code, it only provides analysis (errors and
    /// warnings) of the compiler.
    pub fn compile_only(
        mut self,
        code: impl AsRef<str>,
        location: Option<PathBuf>,
    ) -> EvaluationResult {
        let mut result = EvaluationResult::default();
        let source = self.source_map();

        let location_str = location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "[code]".into());

        let file = source.add_file(location_str, code.as_ref().to_string());

        let mut noop_observer = observer::NoOpObserver::default();
        let compiler_observer = self.compiler_observer.take().unwrap_or(&mut noop_observer);

        parse_compile_internal(
            &mut result,
            code.as_ref(),
            file,
            location,
            source,
            self.globals,
            self.env,
            compiler_observer,
        );

        result
    }

    /// Evaluate the provided source code, at an optional location of the source
    /// code (i.e. path to the file it was read from; used for error reporting,
    /// and for resolving relative paths in impure functions)
    pub fn evaluate(
        mut self,
        code: impl AsRef<str>,
        location: Option<PathBuf>,
    ) -> EvaluationResult {
        let mut result = EvaluationResult::default();
        let source = self.source_map();

        let location_str = location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "[code]".into());

        let file = source.add_file(location_str, code.as_ref().to_string());

        let mut noop_observer = observer::NoOpObserver::default();
        let compiler_observer = self.compiler_observer.take().unwrap_or(&mut noop_observer);

        let lambda = match parse_compile_internal(
            &mut result,
            code.as_ref(),
            file.clone(),
            location,
            source.clone(),
            self.globals.clone(),
            self.env,
            compiler_observer,
        ) {
            None => return result,
            Some(cr) => cr,
        };

        // If bytecode was returned, there were no errors and the
        // code is safe to execute.

        let nix_path = self
            .nix_path
            .as_ref()
            .and_then(|s| match nix_search_path::NixSearchPath::from_str(s) {
                Ok(path) => Some(path),
                Err(err) => {
                    result.warnings.push(EvalWarning {
                        kind: WarningKind::InvalidNixPath(err.to_string()),
                        span: file.span,
                    });
                    None
                }
            })
            .unwrap_or_default();

        let runtime_observer = self.runtime_observer.take().unwrap_or(&mut noop_observer);

        let vm_result = run_lambda(
            nix_path,
            self.io_handle,
            runtime_observer,
            source.clone(),
            self.globals,
            lambda,
            self.strict,
        );

        match vm_result {
            Ok(mut runtime_result) => {
                result.warnings.append(&mut runtime_result.warnings);
                if let Value::Catchable(inner) = runtime_result.value {
                    result.errors.push(Error::new(
                        ErrorKind::CatchableError(*inner),
                        file.span,
                        source,
                    ));
                    return result;
                }

                result.value = Some(runtime_result.value);
            }
            Err(err) => {
                result.errors.push(err);
            }
        }

        result
    }
}

/// Internal helper function for common parsing & compilation logic
/// between the public functions.
#[allow(clippy::too_many_arguments)] // internal API, no point making an indirection type
fn parse_compile_internal(
    result: &mut EvaluationResult,
    code: &str,
    file: Arc<codemap::File>,
    location: Option<PathBuf>,
    source: SourceCode,
    globals: Rc<GlobalsMap>,
    env: Option<&HashMap<SmolStr, Value>>,
    compiler_observer: &mut dyn CompilerObserver,
) -> Option<Rc<Lambda>> {
    let parsed = rnix::ast::Root::parse(code);
    let parse_errors = parsed.errors();

    if !parse_errors.is_empty() {
        result.errors.push(Error::new(
            ErrorKind::ParseErrors(parse_errors.to_vec()),
            file.span,
            source,
        ));
        return None;
    }

    // At this point we know that the code is free of parse errors and
    // we can continue to compile it. The expression is persisted in
    // the result, in case the caller needs it for something.
    result.expr = parsed.tree().expr();

    let compiler_result = match compiler::compile(
        result.expr.as_ref().unwrap(),
        location,
        globals,
        env,
        &source,
        &file,
        compiler_observer,
    ) {
        Ok(result) => result,
        Err(err) => {
            result.errors.push(err);
            return None;
        }
    };

    result.warnings = compiler_result.warnings;
    result.errors.extend(compiler_result.errors);

    // Short-circuit if errors exist at this point (do not pass broken
    // bytecode to the runtime).
    if !result.errors.is_empty() {
        return None;
    }

    // Return the lambda (for execution) and the globals map (to
    // ensure the invariant that the globals outlive the runtime).
    Some(compiler_result.lambda)
}
