use builtin_macros::builtins;
use genawaiter::rc::Gen;

use std::{
    env,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    self as tvix_eval,
    errors::ErrorKind,
    io::FileType,
    value::NixAttrs,
    vm::generators::{self, GenCo},
    NixString, Value,
};

#[builtins]
mod impure_builtins {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use super::*;
    use crate::builtins::{coerce_value_to_path, hash::hash_nix_string};

    #[builtin("getEnv")]
    async fn builtin_get_env(co: GenCo, var: Value) -> Result<Value, ErrorKind> {
        Ok(env::var(OsStr::from_bytes(&var.to_str()?))
            .unwrap_or_else(|_| "".into())
            .into())
    }

    #[builtin("hashFile")]
    #[allow(non_snake_case)]
    async fn builtin_hashFile(co: GenCo, algo: Value, path: Value) -> Result<Value, ErrorKind> {
        let path = match coerce_value_to_path(&co, path).await? {
            Err(cek) => return Ok(Value::from(cek)),
            Ok(p) => p,
        };
        let s = generators::request_read_to_string(&co, path).await;
        hash_nix_string(algo.to_str()?, s.to_str()?).map(Value::from)
    }

    #[builtin("pathExists")]
    async fn builtin_path_exists(co: GenCo, path: Value) -> Result<Value, ErrorKind> {
        match coerce_value_to_path(&co, path).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok(path) => Ok(generators::request_path_exists(&co, path).await),
        }
    }

    #[builtin("readDir")]
    async fn builtin_read_dir(co: GenCo, path: Value) -> Result<Value, ErrorKind> {
        match coerce_value_to_path(&co, path).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok(path) => {
                let dir = generators::request_read_dir(&co, path).await;
                let res = dir.into_iter().map(|(name, ftype)| {
                    (
                        // TODO: propagate Vec<u8> or bytes::Bytes into NixString.
                        NixString::from(
                            String::from_utf8(name.to_vec()).expect("parsing file name as string"),
                        ),
                        Value::from(match ftype {
                            FileType::Directory => "directory",
                            FileType::Regular => "regular",
                            FileType::Symlink => "symlink",
                            FileType::Unknown => "unknown",
                        }),
                    )
                });

                Ok(Value::attrs(NixAttrs::from_iter(res)))
            }
        }
    }

    #[builtin("readFile")]
    async fn builtin_read_file(co: GenCo, path: Value) -> Result<Value, ErrorKind> {
        match coerce_value_to_path(&co, path).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok(path) => Ok(generators::request_read_to_string(&co, path).await),
        }
    }
}

/// Return all impure builtins, that is all builtins which may perform I/O
/// outside of the VM and so cannot be used in all contexts (e.g. WASM).
pub fn impure_builtins() -> Vec<(&'static str, Value)> {
    let mut result = impure_builtins::builtins();

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
