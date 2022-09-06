use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    io,
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    errors::ErrorKind,
    observer::NoOpObserver,
    value::{Builtin, NixAttrs, NixString, Thunk},
    vm::VM,
    SourceCode, Value,
};

fn impure_builtins() -> Vec<Builtin> {
    vec![Builtin::new(
        "readDir",
        &[true],
        |args: Vec<Value>, vm: &mut VM| {
            let path = super::coerce_value_to_path(&args[0], vm)?;
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
        },
    )]
}

/// Return all impure builtins, that is all builtins which may perform I/O
/// outside of the VM and so cannot be used in all contexts (e.g. WASM).
pub(super) fn builtins() -> BTreeMap<NixString, Value> {
    let mut map: BTreeMap<NixString, Value> = impure_builtins()
        .into_iter()
        .map(|b| (b.name().into(), Value::Builtin(b)))
        .collect();

    // currentTime pins the time at which evaluation was started
    {
        let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(dur) => dur.as_secs() as i64,

            // This case is hit if the system time is *before* epoch.
            Err(err) => -(err.duration().as_secs() as i64),
        };

        map.insert(NixString::from("currentTime"), Value::Integer(seconds));
    }

    map
}

/// Constructs and inserts the `import` builtin. This builtin is special in that
/// it needs to capture the [crate::SourceCode] structure to correctly track
/// source code locations while invoking a compiler.
// TODO: need to be able to pass through a CompilationObserver, too.
pub fn builtins_import(
    globals: Rc<RefCell<HashMap<&'static str, Value>>>,
    source: SourceCode,
) -> Builtin {
    Builtin::new(
        "import",
        &[true],
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

            let result = crate::compile(
                &parsed.tree().expr().unwrap(),
                Some(path.clone()),
                file,
                globals.clone(),
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
            Ok(Value::Thunk(Thunk::new(result.lambda)))
        },
    )
}
