use crate::*;

#[test]
fn test_source_builtin() {
    // Test an evaluation with a source-only builtin. The test ensures
    // that the artificially constructed thunking is correct.

    let eval = Evaluation::builder_pure()
        .add_src_builtin("testSourceBuiltin", "42")
        .build();

    let result = eval.evaluate("builtins.testSourceBuiltin", None);
    assert!(
        result.errors.is_empty(),
        "evaluation failed: {:?}",
        result.errors
    );

    let value = result.value.unwrap();
    assert!(
        matches!(value, Value::Integer(42)),
        "expected the integer 42, but got {}",
        value,
    );
}

#[test]
fn skip_broken_bytecode() {
    let result = Evaluation::builder_pure()
        .build()
        .evaluate(/* code = */ "x", None);

    assert_eq!(result.errors.len(), 1);

    assert!(matches!(
        result.errors[0].kind,
        ErrorKind::UnknownStaticVariable
    ));
}
