use crate::eval::interpret;

use test_generator::test_resources;

// eval-okay-* tests contain a snippet of Nix code, and an expectation
// of the produced string output of the evaluator.
//
// These evaluations are always supposed to succeed, i.e. all snippets
// are guaranteed to be valid Nix code.
#[test_resources("src/tests/nix_tests/eval-okay-*.nix")]
fn eval_okay(code_path: &str) {
    let base = code_path
        .strip_suffix("nix")
        .expect("test files always end in .nix");
    let exp_path = format!("{}exp", base);

    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");
    let exp = std::fs::read_to_string(exp_path).expect("should be able to read test expectation");

    let result = interpret(&code).expect("evaluation of eval-okay test should succeed");
    let result_str = format!("{}", result);

    assert_eq!(
        exp.trim(),
        result_str,
        "result value (and its representation) must match expectation"
    );
}
