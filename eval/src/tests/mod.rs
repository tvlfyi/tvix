use crate::eval::interpret;
use pretty_assertions::assert_eq;

use test_generator::test_resources;

fn eval_okay_test(code_path: &str) {
    let base = code_path
        .strip_suffix("nix")
        .expect("test files always end in .nix");
    let exp_path = format!("{}exp", base);

    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");
    let exp = std::fs::read_to_string(exp_path).expect("should be able to read test expectation");

    let result = interpret(&code, None).expect("evaluation of eval-okay test should succeed");
    let result_str = format!("{}", result);

    assert_eq!(
        result_str,
        exp.trim(),
        "result value representation (left) must match expectation (right)"
    );
}

// identity-* tests contain Nix code snippets which should evaluate to
// themselves exactly (i.e. literals).
#[test_resources("src/tests/tvix_tests/identity-*.nix")]
fn identity(code_path: &str) {
    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");

    let result = interpret(&code, None).expect("evaluation of identity test should succeed");
    let result_str = format!("{}", result);

    assert_eq!(
        result_str,
        code.trim(),
        "result value representation (left) must match expectation (right)"
    )
}

// eval-okay-* tests contain a snippet of Nix code, and an expectation
// of the produced string output of the evaluator.
//
// These evaluations are always supposed to succeed, i.e. all snippets
// are guaranteed to be valid Nix code.
#[test_resources("src/tests/tvix_tests/eval-okay-*.nix")]
fn eval_okay(code_path: &str) {
    eval_okay_test(code_path)
}

// eval-okay-* tests from the original Nix test suite.
#[cfg(feature = "nix_tests")]
#[test_resources("src/tests/nix_tests/eval-okay-*.nix")]
fn nix_eval_okay(code_path: &str) {
    eval_okay_test(code_path)
}
