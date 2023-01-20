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
pub use crate::errors::{AddContext, Error, ErrorKind, EvalResult};
pub use crate::io::{DummyIO, EvalIO, FileType};
pub use crate::pretty_ast::pretty_print_expr;
pub use crate::source::SourceCode;
pub use crate::vm::VM;
pub use crate::warnings::{EvalWarning, WarningKind};
pub use builtin_macros;

pub use crate::value::{
    Builtin, BuiltinArgument, CoercionKind, NixAttrs, NixList, NixString, Value,
};

#[cfg(feature = "impure")]
pub use crate::io::StdIO;

/// An `Evaluation` represents how a piece of Nix code is evaluated. It can be
/// instantiated and configured directly, or it can be accessed through the
/// various simplified helper methods available below.
///
/// Public fields are intended to be set by the caller. Setting all
/// fields is optional.
pub struct Evaluation<'code, 'co, 'ro> {
    /// The Nix source code to be evaluated.
    code: &'code str,

    /// Optional location of the source code (i.e. path to the file it was read
    /// from). Used for error reporting, and for resolving relative paths in
    /// impure functions.
    location: Option<PathBuf>,

    /// Source code map used for error reporting.
    source_map: SourceCode,

    /// Top-level file reference for this code inside the source map.
    file: Arc<codemap::File>,

    /// Set of all builtins that should be available during the
    /// evaluation.
    ///
    /// This defaults to all pure builtins. Users might want to add
    /// the set of impure builtins, or other custom builtins.
    pub builtins: Vec<(&'static str, Value)>,

    /// Implementation of file-IO to use during evaluation, e.g. for
    /// impure builtins.
    ///
    /// Defaults to [`DummyIO`] if not set explicitly.
    pub io_handle: Box<dyn EvalIO>,

    /// Determines whether the `import` builtin should be made
    /// available. Note that this depends on the `io_handle` being
    /// able to read the files specified as arguments to `import`.
    pub enable_import: bool,

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

impl<'code, 'co, 'ro> Evaluation<'code, 'co, 'ro> {
    /// Initialise an `Evaluation` for the given Nix source code snippet, and
    /// an optional code location.
    pub fn new(code: &'code str, location: Option<PathBuf>) -> Self {
        let source_map = SourceCode::new();

        let location_str = location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "[code]".into());

        let file = source_map.add_file(location_str, code.into());

        let mut builtins = builtins::pure_builtins();
        builtins.extend(builtins::placeholders()); // these are temporary

        Evaluation {
            code,
            location,
            source_map,
            file,
            builtins,
            io_handle: Box::new(DummyIO {}),
            enable_import: false,
            nix_path: None,
            compiler_observer: None,
            runtime_observer: None,
        }
    }

    #[cfg(feature = "impure")]
    /// Initialise an `Evaluation` for the given snippet, with all
    /// impure features turned on by default.
    pub fn new_impure(code: &'code str, location: Option<PathBuf>) -> Self {
        let mut eval = Self::new(code, location);
        eval.enable_import = true;
        eval.builtins.extend(builtins::impure_builtins());
        eval.io_handle = Box::new(StdIO);
        eval
    }

    /// Clone the reference to the contained source code map. This is used after
    /// an evaluation for pretty error printing.
    pub fn source_map(&self) -> SourceCode {
        self.source_map.clone()
    }

    /// Only compile the provided source code. This does not *run* the
    /// code, it only provides analysis (errors and warnings) of the
    /// compiler.
    pub fn compile_only(mut self) -> EvaluationResult {
        let mut result = EvaluationResult::default();
        let source = self.source_map();

        let mut noop_observer = observer::NoOpObserver::default();
        let compiler_observer = self.compiler_observer.take().unwrap_or(&mut noop_observer);

        parse_compile_internal(
            &mut result,
            self.code,
            self.file.clone(),
            self.location,
            source,
            self.builtins,
            self.enable_import,
            compiler_observer,
        );

        result
    }

    /// Evaluate the provided source code.
    pub fn evaluate(mut self) -> EvaluationResult {
        let mut result = EvaluationResult::default();
        let source = self.source_map();

        let mut noop_observer = observer::NoOpObserver::default();
        let compiler_observer = self.compiler_observer.take().unwrap_or(&mut noop_observer);

        let (lambda, globals) = match parse_compile_internal(
            &mut result,
            self.code,
            self.file.clone(),
            self.location,
            source,
            self.builtins,
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
                        span: self.file.span,
                    });
                    None
                }
            })
            .unwrap_or_default();

        let runtime_observer = self.runtime_observer.take().unwrap_or(&mut noop_observer);
        let vm_result = run_lambda(nix_path, self.io_handle, runtime_observer, globals, lambda);

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
fn parse_compile_internal(
    result: &mut EvaluationResult,
    code: &str,
    file: Arc<codemap::File>,
    location: Option<PathBuf>,
    source: SourceCode,
    builtins: Vec<(&'static str, Value)>,
    enable_import: bool,
    compiler_observer: &mut dyn CompilerObserver,
) -> Option<(Rc<Lambda>, Rc<GlobalsMap>)> {
    let parsed = rnix::ast::Root::parse(code);
    let parse_errors = parsed.errors();

    if !parse_errors.is_empty() {
        result.errors.push(Error::new(
            ErrorKind::ParseErrors(parse_errors.to_vec()),
            file.span,
        ));
        return None;
    }

    // At this point we know that the code is free of parse errors and
    // we can continue to compile it. The expression is persisted in
    // the result, in case the caller needs it for something.
    result.expr = parsed.tree().expr();

    let builtins = crate::compiler::prepare_globals(builtins, source, enable_import);

    let compiler_result = match compiler::compile(
        result.expr.as_ref().unwrap(),
        location,
        file.clone(),
        builtins,
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

    // Return the lambda (for execution) and the globals map (to
    // ensure the invariant that the globals outlive the runtime).
    Some((compiler_result.lambda, compiler_result.globals))
}
