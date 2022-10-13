use crate::eval::interpret;
use crate::eval::Options;
use pretty_assertions::assert_eq;

use test_generator::test_resources;

fn eval_test(code_path: &str, expect_success: bool) {
    let base = code_path
        .strip_suffix("nix")
        .expect("test files always end in .nix");
    let exp_path = format!("{}exp", base);

    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");

    match interpret(&code, Some(code_path.into()), Options::test_options()) {
        Ok(result) => {
            if !expect_success {
                panic!(
                    "test should have failed, but succeeded with output {}",
                    result
                );
            }
            let result_str = format!("{}", result);
            let exp =
                std::fs::read_to_string(exp_path).expect("should be able to read test expectation");
            assert_eq!(
                result_str,
                exp.trim(),
                "result value representation (left) must match expectation (right)"
            );
        }
        Err(e) => {
            if expect_success {
                panic!(
                    "evaluation of eval-okay test should succeed, but failed with {:?}",
                    e
                );
            }
        }
    }
}

// identity-* tests contain Nix code snippets which should evaluate to
// themselves exactly (i.e. literals).
#[test_resources("src/tests/tvix_tests/identity-*.nix")]
fn identity(code_path: &str) {
    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");

    let result = interpret(&code, None, Options::test_options())
        .expect("evaluation of identity test should succeed");
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
    eval_test(code_path, true)
}

// eval-fail-* tests from the original Nix test suite.
#[cfg(feature = "nix_tests")]
#[test_resources("src/tests/nix_tests/eval-okay-*.nix")]
fn nix_eval_okay(code_path: &str) {
    eval_test(code_path, true)
}

// eval-fail-* tests contain a snippet of Nix code, which is
// expected to fail evaluation.  The exact type of failure
// (assertion, parse error, etc) is not currently checked.
#[test_resources("src/tests/tvix_tests/eval-fail-*.nix")]
fn eval_fail(code_path: &str) {
    eval_test(code_path, false)
}

// eval-fail-* tests from the original Nix test suite.
#[cfg(feature = "nix_tests")]
#[test_resources("src/tests/nix_tests/eval-fail-*.nix")]
fn nix_eval_fail(code_path: &str) {
    eval_test(code_path, false)
}
