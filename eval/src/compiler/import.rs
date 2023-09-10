//! This module implements the Nix language's `import` feature, which
//! is exposed as a builtin in the Nix language.
//!
//! This is not a typical builtin, as it needs access to internal
//! compiler and VM state (such as the [`crate::SourceCode`]
//! instance, or observers).

use super::GlobalsMap;
use genawaiter::rc::Gen;
use std::rc::Weak;

use crate::{
    builtins::coerce_value_to_path,
    generators::pin_generator,
    observer::NoOpObserver,
    value::{Builtin, Thunk},
    vm::generators::{self, GenCo},
    ErrorKind, SourceCode, Value,
};

async fn import_impl(
    co: GenCo,
    globals: Weak<GlobalsMap>,
    source: SourceCode,
    mut args: Vec<Value>,
) -> Result<Value, ErrorKind> {
    // TODO(sterni): canon_path()?
    let mut path = match coerce_value_to_path(&co, args.pop().unwrap()).await? {
        Err(cek) => return Ok(Value::Catchable(cek)),
        Ok(path) => path,
    };

    if path.is_dir() {
        path.push("default.nix");
    }

    if let Some(cached) = generators::request_import_cache_lookup(&co, path.clone()).await {
        return Ok(cached);
    }

    // TODO(tazjin): make this return a string directly instead
    let contents: Value = generators::request_read_to_string(&co, path.clone()).await;
    let contents = contents.to_str()?.as_str().to_string();

    let parsed = rnix::ast::Root::parse(&contents);
    let errors = parsed.errors();
    let file = source.add_file(path.to_string_lossy().to_string(), contents);

    if !errors.is_empty() {
        return Err(ErrorKind::ImportParseError {
            path,
            file,
            errors: errors.to_vec(),
        });
    }

    let result = crate::compiler::compile(
        &parsed.tree().expr().unwrap(),
        Some(path.clone()),
        file,
        // The VM must ensure that a strong reference to the globals outlives
        // any self-references (which are weak) embedded within the globals. If
        // the expect() below panics, it means that did not happen.
        globals
            .upgrade()
            .expect("globals dropped while still in use"),
        &mut NoOpObserver::default(),
    )
    .map_err(|err| ErrorKind::ImportCompilerError {
        path: path.clone(),
        errors: vec![err],
    })?;

    if !result.errors.is_empty() {
        return Err(ErrorKind::ImportCompilerError {
            path,
            errors: result.errors,
        });
    }

    for warning in result.warnings {
        generators::emit_warning(&co, warning).await;
    }

    // Compilation succeeded, we can construct a thunk from whatever it spat
    // out and return that.
    let res = Value::Thunk(Thunk::new_suspended(
        result.lambda,
        generators::request_span(&co).await,
    ));

    generators::request_import_cache_put(&co, path, res.clone()).await;

    Ok(res)
}

/// Constructs the `import` builtin. This builtin is special in that
/// it needs to capture the [crate::SourceCode] structure to correctly
/// track source code locations while invoking a compiler.
// TODO: need to be able to pass through a CompilationObserver, too.
// TODO: can the `SourceCode` come from the compiler?
pub(super) fn builtins_import(globals: &Weak<GlobalsMap>, source: SourceCode) -> Builtin {
    // This (very cheap, once-per-compiler-startup) clone exists
    // solely in order to keep the borrow checker happy.  It
    // resolves the tension between the requirements of
    // Rc::new_cyclic() and Builtin::new()
    let globals = globals.clone();

    Builtin::new(
        "import",
        Some("Import the given file and return the Nix value it evaluates to"),
        1,
        move |args| {
            Gen::new(|co| pin_generator(import_impl(co, globals.clone(), source.clone(), args)))
        },
    )
}
