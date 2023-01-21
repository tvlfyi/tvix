use crate::*;

#[test]
fn test_source_builtin() {
    // Test an evaluation with a source-only builtin. The test ensures
    // that the artificially constructed thunking is correct.

    let mut eval = Evaluation::new_impure("builtins.testSourceBuiltin", None);
    eval.src_builtins.push(("testSourceBuiltin", "42"));

    let result = eval.evaluate();
    assert!(
        result.errors.is_empty(),
        "evaluation failed: {:?}",
        result.errors
    );

    assert!(matches!(result.value.unwrap(), Value::Integer(42)));
}
