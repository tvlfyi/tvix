pub use tvix_eval::internal;
pub use tvix_eval::Value;
use tvix_eval_builtin_macros::builtins;

#[builtins]
mod builtins {
    use tvix_eval::internal::VM;
    use tvix_eval::{ErrorKind, Value};

    #[builtin("identity")]
    pub fn builtin_identity(_vm: &mut VM, x: Value) -> Result<Value, ErrorKind> {
        Ok(x)
    }

    #[builtin("tryEval")]
    pub fn builtin_try_eval(_: &mut VM, #[lazy] _x: Value) -> Result<Value, ErrorKind> {
        todo!()
    }
}

#[test]
fn builtins() {
    let builtins = builtins::builtins();
    assert_eq!(builtins.len(), 2);
}
