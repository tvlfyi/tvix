//! This module implements the Nix language's `import` feature, which
//! is exposed as a builtin in the Nix language.
//!
//! This is not a typical builtin, as it needs access to internal
//! compiler and VM state (such as the [`crate::SourceCode`]
//! instance, or observers).

use std::rc::Weak;

use crate::{
    observer::NoOpObserver,
    value::{Builtin, BuiltinArgument, Thunk},
    vm::VM,
    ErrorKind, SourceCode, Value,
};

use super::GlobalsMap;
use crate::builtins::coerce_value_to_path;

/// Constructs and inserts the `import` builtin. This builtin is special in that
/// it needs to capture the [crate::SourceCode] structure to correctly track
/// source code locations while invoking a compiler.
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
        &[BuiltinArgument {
            strict: true,
            name: "path",
        }],
        None,
        move |mut args: Vec<Value>, vm: &mut VM| {
            let mut path = coerce_value_to_path(&args.pop().unwrap(), vm)?;
            if path.is_dir() {
                path.push("default.nix");
            }

            let current_span = vm.current_light_span();

            if let Some(cached) = vm.import_cache.get(&path) {
                return Ok(cached.clone());
            }

            let contents = vm.io().read_to_string(path.clone())?;

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
                // The VM must ensure that a strong reference to the
                // globals outlives any self-references (which are
                // weak) embedded within the globals.  If the
                // expect() below panics, it means that did not
                // happen.
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

            // Compilation succeeded, we can construct a thunk from whatever it spat
            // out and return that.
            let res = Value::Thunk(Thunk::new_suspended(result.lambda, current_span));

            vm.import_cache.insert(path, res.clone());

            for warning in result.warnings {
                vm.push_warning(warning);
            }

            Ok(res)
        },
    )
}
