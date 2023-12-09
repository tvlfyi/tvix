pub use tvix_eval::{Builtin, Value};
use tvix_eval_builtin_macros::builtins;

#[builtins]
mod builtins {
    use tvix_eval::generators::{Gen, GenCo};
    use tvix_eval::{ErrorKind, Value};

    /// Test docstring.
    ///
    /// It has multiple lines!
    #[builtin("identity")]
    pub async fn builtin_identity(co: GenCo, x: Value) -> Result<Value, ErrorKind> {
        Ok(x)
    }

    #[builtin("tryEval")]
    pub async fn builtin_try_eval(_co: GenCo, #[lazy] _x: Value) -> Result<Value, ErrorKind> {
        unimplemented!("builtin is never called")
    }
}

#[test]
fn builtins() {
    let builtins = builtins::builtins();
    assert_eq!(builtins.len(), 2);

    let (_, identity) = builtins
        .iter()
        .find(|(name, _)| *name == "identity")
        .unwrap();

    match identity {
        Value::Builtin(identity) => assert_eq!(
            identity.documentation(),
            Some(
                r#" Test docstring.

 It has multiple lines!"#
            )
        ),

        _ => panic!("builtin was not a builtin"),
    }
}
