use builtin_macros::builtins;
use smol_str::SmolStr;

use std::{
    env,
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    errors::ErrorKind,
    io::FileType,
    value::{NixAttrs, Thunk},
    vm::VM,
    Value,
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
        let path = coerce_value_to_path(&s, vm)?;
        vm.io().path_exists(path).map(Value::Bool)
    }

    #[builtin("readDir")]
    fn builtin_read_dir(vm: &mut VM, path: Value) -> Result<Value, ErrorKind> {
        let path = coerce_value_to_path(&path, vm)?;

        let res = vm.io().read_dir(path)?.into_iter().map(|(name, ftype)| {
            (
                name,
                Value::String(
                    SmolStr::new(match ftype {
                        FileType::Directory => "directory",
                        FileType::Regular => "regular",
                        FileType::Symlink => "symlink",
                        FileType::Unknown => "unknown",
                    })
                    .into(),
                ),
            )
        });

        Ok(Value::attrs(NixAttrs::from_iter(res)))
    }

    #[builtin("readFile")]
    fn builtin_read_file(vm: &mut VM, path: Value) -> Result<Value, ErrorKind> {
        let path = coerce_value_to_path(&path, vm)?;
        vm.io()
            .read_to_string(path)
            .map(|s| Value::String(s.into()))
    }
}

/// Return all impure builtins, that is all builtins which may perform I/O
/// outside of the VM and so cannot be used in all contexts (e.g. WASM).
pub fn impure_builtins() -> Vec<(&'static str, Value)> {
    let mut result = impure_builtins::builtins();

    result.push((
        "storeDir",
        Value::Thunk(Thunk::new_suspended_native(Rc::new(
            |vm: &mut VM| match vm.io().store_dir() {
                None => Ok(Value::Null),
                Some(dir) => Ok(Value::String(dir.into())),
            },
        ))),
    ));

    // currentTime pins the time at which evaluation was started
    {
        let seconds = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(dur) => dur.as_secs() as i64,

            // This case is hit if the system time is *before* epoch.
            Err(err) => -(err.duration().as_secs() as i64),
        };

        result.push(("currentTime", Value::Integer(seconds)));
    }

    result
}
