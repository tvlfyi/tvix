use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    errors::ErrorKind,
    observer::NoOpObserver,
    value::{Builtin, NixString, Thunk},
    vm::VM,
    SourceCode, Value,
};

fn impure_builtins() -> Vec<Builtin> {
    vec![]
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
            let path = super::coerce_value_to_path(&args.pop().unwrap(), vm)?;

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
