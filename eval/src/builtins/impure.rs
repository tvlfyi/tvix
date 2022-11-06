use builtin_macros::builtins;
use std::{
    collections::BTreeMap,
    env,
    fs::File,
    io::{self, Read},
    rc::{Rc, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    compiler::GlobalsMap,
    errors::ErrorKind,
    observer::NoOpObserver,
    value::{Builtin, BuiltinArgument, NixAttrs, Thunk},
    vm::VM,
    SourceCode, Value,
};

#[builtins]
mod impure_builtins {
    use super::*;
    use crate::builtins::coerce_value_to_path;

    #[builtin("getEnv")]
    fn builtin_get_env(_: &mut VM, var: Value) -> Result<Value, ErrorKind> {
        Ok(env::var(var.to_str()?).unwrap_or_else(|_| "".into()).into())
    }

    #[builtin("pathExists")]
    fn builtin_path_exists(vm: &mut VM, s: Value) -> Result<Value, ErrorKind> {
        Ok(coerce_value_to_path(&s, vm)?.exists().into())
    }

    #[builtin("readDir")]
    fn builtin_read_dir(vm: &mut VM, path: Value) -> Result<Value, ErrorKind> {
        let path = coerce_value_to_path(&path, vm)?;
        let mk_err = |err: io::Error| ErrorKind::IO {
            path: Some(path.clone()),
            error: Rc::new(err),
        };

        let mut res = BTreeMap::new();
        for entry in path.read_dir().map_err(mk_err)? {
            let entry = entry.map_err(mk_err)?;
            let file_type = entry
                .metadata()
                .map_err(|err| ErrorKind::IO {
                    path: Some(entry.path()),
                    error: Rc::new(err),
                })?
                .file_type();
            let val = if file_type.is_dir() {
                "directory"
            } else if file_type.is_file() {
                "regular"
            } else if file_type.is_symlink() {
                "symlink"
            } else {
                "unknown"
            };
            res.insert(
                entry.file_name().to_string_lossy().as_ref().into(),
                val.into(),
            );
        }
        Ok(Value::attrs(NixAttrs::from_map(res)))
    }

    #[builtin("readFile")]
    fn builtin_read_file(vm: &mut VM, path: Value) -> Result<Value, ErrorKind> {
        let mut buf = String::new();
        File::open(&coerce_value_to_path(&path, vm)?)?.read_to_string(&mut buf)?;
        Ok(buf.into())
    }
}

/// Return all impure builtins, that is all builtins which may perform I/O
/// outside of the VM and so cannot be used in all contexts (e.g. WASM).
pub(super) fn builtins() -> BTreeMap<&'static str, Value> {
    let mut map: BTreeMap<&'static str, Value> = impure_builtins::builtins()
        .into_iter()
        .map(|b| (b.name(), Value::Builtin(b)))
        .collect();

    // currentTime pins the time at which evaluation was started
    {
        let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(dur) => dur.as_secs() as i64,

            // This case is hit if the system time is *before* epoch.
            Err(err) => -(err.duration().as_secs() as i64),
        };

        map.insert("currentTime", Value::Integer(seconds));
    }

    map
}

/// Constructs and inserts the `import` builtin. This builtin is special in that
/// it needs to capture the [crate::SourceCode] structure to correctly track
/// source code locations while invoking a compiler.
// TODO: need to be able to pass through a CompilationObserver, too.
pub fn builtins_import(globals: &Weak<GlobalsMap>, source: SourceCode) -> Builtin {
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
            let mut path = super::coerce_value_to_path(&args.pop().unwrap(), vm)?;
            if path.is_dir() {
                path.push("default.nix");
            }

            let contents =
                std::fs::read_to_string(&path).map_err(|err| ErrorKind::ReadFileError {
                    path: path.clone(),
                    error: Rc::new(err),
                })?;

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

            for warning in result.warnings {
                vm.push_warning(warning);
            }

            // Compilation succeeded, we can construct a thunk from whatever it spat
            // out and return that.
            Ok(Value::Thunk(Thunk::new_suspended(
                result.lambda,
                vm.current_span(),
            )))
        },
    )
}
