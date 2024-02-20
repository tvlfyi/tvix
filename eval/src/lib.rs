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
#[cfg(test)]
mod properties;
#[cfg(test)]
mod test_utils;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use crate::compiler::GlobalsMap;
use crate::observer::{CompilerObserver, RuntimeObserver};
use crate::value::Lambda;
use crate::vm::run_lambda;

// Re-export the public interface used by other crates.
pub use crate::compiler::{compile, prepare_globals, CompilationOutput};
pub use crate::errors::{AddContext, CatchableErrorKind, Error, ErrorKind, EvalResult};
pub use crate::io::{DummyIO, EvalIO, FileType};
pub use crate::pretty_ast::pretty_print_expr;
pub use crate::source::SourceCode;
pub use crate::value::{NixContext, NixContextElement};
pub use crate::vm::generators;
pub use crate::warnings::{EvalWarning, WarningKind};
pub use builtin_macros;

pub use crate::value::{Builtin, CoercionKind, NixAttrs, NixList, NixString, Value};

#[cfg(feature = "impure")]
pub use crate::io::StdIO;

/// An `Evaluation` represents how a piece of Nix code is evaluated. It can be
/// instantiated and configured directly, or it can be accessed through the
/// various simplified helper methods available below.
///
/// Public fields are intended to be set by the caller. Setting all
/// fields is optional.
pub struct Evaluation<'co, 'ro, IO> {
    /// Source code map used for error reporting.
    source_map: SourceCode,

    /// Set of all builtins that should be available during the
    /// evaluation.
    ///
    /// This defaults to all pure builtins. Users might want to add
    /// the set of impure builtins, or other custom builtins.
    pub builtins: Vec<(&'static str, Value)>,

    /// Set of builtins that are implemented in Nix itself and should
    /// be compiled and inserted in the builtins set.
    pub src_builtins: Vec<(&'static str, &'static str)>,

    /// Implementation of file-IO to use during evaluation, e.g. for
    /// impure builtins.
    ///
    /// Defaults to [`DummyIO`] if not set explicitly.
    pub io_handle: IO,

    /// Determines whether the `import` builtin should be made
    /// available. Note that this depends on the `io_handle` being
    /// able to read the files specified as arguments to `import`.
    pub enable_import: bool,

    /// Determines whether the returned value should be strictly
    /// evaluated, that is whether its list and attribute set elements
    /// should be forced recursively.
    pub strict: bool,

    /// (optional) Nix search path, e.g. the value of `NIX_PATH` used
    /// for resolving items on the search path (such as `<nixpkgs>`).
    pub nix_path: Option<String>,

    /// (optional) compiler observer for reporting on compilation
    /// details, like the emitted bytecode.
    pub compiler_observer: Option<&'co mut dyn CompilerObserver>,

    /// (optional) runtime observer, for reporting on execution steps
    /// of Nix code.
    pub runtime_observer: Option<&'ro mut dyn RuntimeObserver>,
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

impl<'co, 'ro, IO> Evaluation<'co, 'ro, IO>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    /// Initialize an `Evaluation`.
    pub fn new(io_handle: IO, enable_import: bool) -> Self {
        let mut builtins = builtins::pure_builtins();
        builtins.extend(builtins::placeholders()); // these are temporary

        Self {
            source_map: SourceCode::default(),
            enable_import,
            io_handle,
            builtins,
            src_builtins: vec![],
            strict: false,
            nix_path: None,
            compiler_observer: None,
            runtime_observer: None,
        }
    }
}

impl<'co, 'ro> Evaluation<'co, 'ro, Box<dyn EvalIO>> {
    /// Initialize an `Evaluation`, without the import statement available, and
    /// all IO operations stubbed out.
    pub fn new_pure() -> Self {
        Self::new(Box::new(DummyIO) as Box<dyn EvalIO>, false)
    }

    #[cfg(feature = "impure")]
    /// Configure an `Evaluation` to have impure features available
    /// with the given I/O implementation.
    ///
    /// If no I/O implementation is supplied, [`StdIO`] is used by
    /// default.
    pub fn enable_impure(&mut self, io: Option<Box<dyn EvalIO>>) {
        self.io_handle = io.unwrap_or_else(|| Box::new(StdIO) as Box<dyn EvalIO>);
        self.enable_import = true;
        self.builtins.extend(builtins::impure_builtins());
    }

    #[cfg(feature = "impure")]
    /// Initialise an `Evaluation`, with all impure features turned on by default.
    pub fn new_impure() -> Self {
        let mut eval = Self::new_pure();
        eval.enable_impure(None);
        eval
    }
}

impl<'co, 'ro, IO> Evaluation<'co, 'ro, IO>
where
    IO: AsRef<dyn EvalIO> + 'static,
{
    /// Clone the reference to the contained source code map. This is used after
    /// an evaluation for pretty error printing.
    pub fn source_map(&self) -> SourceCode {
        self.source_map.clone()
    }

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
            self.builtins,
            self.src_builtins,
            self.enable_import,
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

        // Insert a storeDir builtin *iff* a store directory is present.
        if let Some(store_dir) = self.io_handle.as_ref().store_dir() {
            self.builtins.push(("storeDir", store_dir.into()));
        }

        let (lambda, globals) = match parse_compile_internal(
            &mut result,
            code.as_ref(),
            file.clone(),
            location,
            source.clone(),
            self.builtins,
            self.src_builtins,
            self.enable_import,
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
            source,
            globals,
            lambda,
            self.strict,
        );

        match vm_result {
            Ok(mut runtime_result) => {
                result.warnings.append(&mut runtime_result.warnings);
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
    builtins: Vec<(&'static str, Value)>,
    src_builtins: Vec<(&'static str, &'static str)>,
    enable_import: bool,
    compiler_observer: &mut dyn CompilerObserver,
) -> Option<(Rc<Lambda>, Rc<GlobalsMap>)> {
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

    let builtins =
        crate::compiler::prepare_globals(builtins, src_builtins, source.clone(), enable_import);

    let compiler_result = match compiler::compile(
        result.expr.as_ref().unwrap(),
        location,
        builtins,
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
    Some((compiler_result.lambda, compiler_result.globals))
}
