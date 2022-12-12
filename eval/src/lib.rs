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

mod builtins;
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

// Re-export the public interface used by other crates.
pub use crate::builtins::global_builtins;
pub use crate::compiler::{compile, prepare_globals};
pub use crate::errors::{Error, ErrorKind, EvalResult};
use crate::observer::{CompilerObserver, RuntimeObserver};
pub use crate::pretty_ast::pretty_print_expr;
pub use crate::source::SourceCode;
pub use crate::value::Value;
pub use crate::vm::run_lambda;
pub use crate::warnings::{EvalWarning, WarningKind};

/// Internal-only parts of `tvix-eval`, exported for use in macros, but not part of the public
/// interface of the crate.
pub mod internal {
    pub use crate::value::{Builtin, BuiltinArgument};
    pub use crate::vm::VM;
}

// TODO: use Rc::unwrap_or_clone once it is stabilised.
// https://doc.rust-lang.org/std/rc/struct.Rc.html#method.unwrap_or_clone
pub(crate) fn unwrap_or_clone_rc<T: Clone>(rc: Rc<T>) -> T {
    Rc::try_unwrap(rc).unwrap_or_else(|rc| (*rc).clone())
}

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
    /// reporting the location of errors in the code.
    pub fn new(code: &'code str, location: Option<PathBuf>) -> Self {
        let source_map = SourceCode::new();

        let location_str = location
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "[code]".into());

        let file = source_map.add_file(location_str, code.into());

        Evaluation {
            code,
            location,
            source_map,
            file,
            nix_path: None,
            compiler_observer: None,
            runtime_observer: None,
        }
    }

    /// Clone the reference to the contained source code map. This is used after
    /// an evaluation for pretty error printing.
    pub fn source_map(&self) -> SourceCode {
        self.source_map.clone()
    }

    /// Evaluate the provided source code.
    pub fn evaluate(mut self) -> EvaluationResult {
        let mut result = EvaluationResult::default();
        let parsed = rnix::ast::Root::parse(self.code);
        let parse_errors = parsed.errors();

        if !parse_errors.is_empty() {
            result.errors.push(Error {
                kind: ErrorKind::ParseErrors(parse_errors.to_vec()),
                span: self.file.span,
            });
            return result;
        }

        // At this point we know that the code is free of parse errors and we
        // can continue to compile it.
        //
        // The root expression is persisted in self in case the caller wants
        // access to the parsed expression.
        result.expr = parsed.tree().expr();

        let builtins =
            crate::compiler::prepare_globals(Box::new(global_builtins(self.source_map())));

        let mut noop_observer = observer::NoOpObserver::default();
        let compiler_observer = self.compiler_observer.take().unwrap_or(&mut noop_observer);

        let compiler_result = match compiler::compile(
            result.expr.as_ref().unwrap(),
            self.location.take(),
            self.file.clone(),
            builtins,
            compiler_observer,
        ) {
            Ok(result) => result,
            Err(err) => {
                result.errors.push(err);
                return result;
            }
        };

        result.warnings = compiler_result.warnings;

        if !compiler_result.errors.is_empty() {
            result.errors = compiler_result.errors;
            return result;
        }

        // If there were no errors during compilation, the resulting bytecode is
        // safe to execute.

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
            .unwrap_or_else(|| Default::default());

        let runtime_observer = self.runtime_observer.take().unwrap_or(&mut noop_observer);
        let vm_result = run_lambda(nix_path, runtime_observer, compiler_result.lambda);

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
