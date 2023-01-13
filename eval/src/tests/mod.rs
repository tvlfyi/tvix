use builtin_macros::builtins;
use pretty_assertions::assert_eq;
use test_generator::test_resources;

#[builtins]
mod mock_builtins {
    //! Builtins which are required by language tests, but should not
    //! actually exist in //tvix/eval.
    use crate::*;

    #[builtin("derivation")]
    fn builtin_derivation(vm: &mut VM, input: Value) -> Result<Value, ErrorKind> {
        vm.emit_warning(WarningKind::NotImplemented("builtins.derivation"));

        let input = input.to_attrs()?;
        let attrs = input.update(NixAttrs::from_iter(
            [
                (
                    "outPath",
                    "/nix/store/00000000000000000000000000000000-mock",
                ),
                (
                    "drvPath",
                    "/nix/store/00000000000000000000000000000000-mock.drv",
                ),
                ("type", "derivation"),
            ]
            .into_iter(),
        ));

        Ok(Value::Attrs(Box::new(attrs)))
    }
}

fn eval_test(code_path: &str, expect_success: bool) {
    let base = code_path
        .strip_suffix("nix")
        .expect("test files always end in .nix");
    let exp_path = format!("{}exp", base);
    let exp_xml_path = std::path::PathBuf::from(format!("{}exp.xml", base));

    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");

    if exp_xml_path.exists() {
        // We can't test them at the moment because we don't have XML output yet.
        // Checking for success / failure only is a bit disingenious.
        return;
    }

    let mut eval = crate::Evaluation::new_impure(&code, Some(code_path.into()));
    eval.builtins.extend(mock_builtins::builtins());

    let result = eval.evaluate();

    if expect_success && !result.errors.is_empty() {
        panic!(
            "{code_path}: evaluation of eval-okay test should succeed, but failed with {:?}",
            result.errors,
        );
    }

    if !expect_success && !result.errors.is_empty() {
        return;
    }

    let result_str = result.value.unwrap().to_string();

    if let Ok(exp) = std::fs::read_to_string(exp_path) {
        if expect_success {
            assert_eq!(
                result_str,
                exp.trim(),
                "{code_path}: result value representation (left) must match expectation (right)"
            );
        } else {
            assert_ne!(
                result_str,
                exp.trim(),
                "{code_path}: test passed unexpectedly!  consider moving it out of notyetpassing"
            );
        }
    } else if expect_success {
        panic!("{code_path}: should be able to read test expectation");
    } else {
        panic!(
            "{code_path}: test should have failed, but succeeded with output {}",
            result_str
        );
    }
}

// identity-* tests contain Nix code snippets which should evaluate to
// themselves exactly (i.e. literals).
#[test_resources("src/tests/tvix_tests/identity-*.nix")]
fn identity(code_path: &str) {
    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");

    let mut eval = crate::Evaluation::new(&code, None);
    eval.io_handle = Box::new(crate::StdIO);

    let result = eval.evaluate();
    assert!(
        result.errors.is_empty(),
        "evaluation of identity test failed: {:?}",
        result.errors
    );

    let result_str = result.value.unwrap().to_string();

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

// eval-okay-* tests from the original Nix test suite.
#[cfg(feature = "nix_tests")]
#[test_resources("src/tests/nix_tests/eval-okay-*.nix")]
fn nix_eval_okay(code_path: &str) {
    eval_test(code_path, true)
}

// eval-okay-* tests from the original Nix test suite which do not yet pass for tvix
//
// Eventually there will be none of these left, and this function
// will disappear :) Until then, to run these tests, use `cargo test
// --features expected_failures`.
//
// Please don't submit failing tests unless they're in
// notyetpassing; this makes the test suite much more useful for
// regression testing, since there should always be zero non-ignored
// failing tests.
//
// Unfortunately test_generator is unmaintained, so the PRs to make
// it understand #[ignored] has been sitting for two years, so we
// can't use `cargo test --include-ignored`, which is the normal way
// of handling this situation.
//
//   https://github.com/frehberg/test-generator/pull/10
//   https://github.com/frehberg/test-generator/pull/8
#[test_resources("src/tests/nix_tests/notyetpassing/eval-okay-*.nix")]
fn nix_eval_okay_currently_failing(code_path: &str) {
    eval_test(code_path, false)
}

#[test_resources("src/tests/tvix_tests/notyetpassing/eval-okay-*.nix")]
fn eval_okay_currently_failing(code_path: &str) {
    eval_test(code_path, false)
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
