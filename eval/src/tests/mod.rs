use crate::{value::Value, EvalIO};
use builtin_macros::builtins;
use pretty_assertions::assert_eq;
use rstest::rstest;
use std::path::PathBuf;

/// Module for one-off tests which do not follow the rest of the
/// test layout.
mod one_offs;

#[builtins]
mod mock_builtins {
    //! Builtins which are required by language tests, but should not
    //! actually exist in //tvix/eval.
    use crate as tvix_eval;
    use crate::generators::GenCo;
    use crate::*;
    use genawaiter::rc::Gen;

    #[builtin("derivation")]
    async fn builtin_derivation(co: GenCo, input: Value) -> Result<Value, ErrorKind> {
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

fn eval_test(code_path: PathBuf, expect_success: bool) {
    eprintln!("path: {}", code_path.display());
    assert_eq!(
        code_path.extension().unwrap(),
        "nix",
        "test files always end in .nix"
    );
    let exp_path = code_path.with_extension("exp");
    let exp_xml_path = code_path.with_extension("exp.xml");

    let code = std::fs::read_to_string(&code_path).expect("should be able to read test code");

    if exp_xml_path.exists() {
        // We can't test them at the moment because we don't have XML output yet.
        // Checking for success / failure only is a bit disingenious.
        return;
    }

    let mut eval = crate::Evaluation::new_impure();
    eval.strict = true;
    eval.builtins.extend(mock_builtins::builtins());

    let result = eval.evaluate(code, Some(code_path.clone()));
    let failed = match result.value {
        Some(Value::Catchable(_)) => true,
        _ => !result.errors.is_empty(),
    };
    if expect_success && failed {
        panic!(
            "{}: evaluation of eval-okay test should succeed, but failed with {:?}",
            code_path.display(),
            result.errors,
        );
    }

    if !expect_success && failed {
        return;
    }

    let value = result.value.unwrap();
    let result_str = value.to_string();

    if let Ok(exp) = std::fs::read_to_string(exp_path) {
        if expect_success {
            assert_eq!(
                result_str,
                exp.trim(),
                "{}: result value representation (left) must match expectation (right)",
                code_path.display()
            );
        } else {
            assert_ne!(
                result_str,
                exp.trim(),
                "{}: test passed unexpectedly!  consider moving it out of notyetpassing",
                code_path.display()
            );
        }
    } else if expect_success {
        panic!(
            "{}: should be able to read test expectation",
            code_path.display()
        );
    } else {
        panic!(
            "{}: test should have failed, but succeeded with output {}",
            code_path.display(),
            result_str
        );
    }
}

// identity-* tests contain Nix code snippets which should evaluate to
// themselves exactly (i.e. literals).
#[rstest]
fn identity(#[files("src/tests/tvix_tests/identity-*.nix")] code_path: PathBuf) {
    let code = std::fs::read_to_string(code_path).expect("should be able to read test code");

    let eval = crate::Evaluation {
        strict: true,
        io_handle: Box::new(crate::StdIO) as Box<dyn EvalIO>,
        ..Default::default()
    };

    let result = eval.evaluate(&code, None);
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
#[rstest]
fn eval_okay(#[files("src/tests/tvix_tests/eval-okay-*.nix")] code_path: PathBuf) {
    eval_test(code_path, true)
}

// eval-okay-* tests from the original Nix test suite.
#[cfg(feature = "nix_tests")]
#[rstest]
fn nix_eval_okay(#[files("src/tests/nix_tests/eval-okay-*.nix")] code_path: PathBuf) {
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
#[rstest]
fn nix_eval_okay_currently_failing(
    #[files("src/tests/nix_tests/notyetpassing/eval-okay-*.nix")] code_path: PathBuf,
) {
    eval_test(code_path, false)
}

#[rstest]
fn eval_okay_currently_failing(
    #[files("src/tests/tvix_tests/notyetpassing/eval-okay-*.nix")] code_path: PathBuf,
) {
    eval_test(code_path, false)
}

// eval-fail-* tests contain a snippet of Nix code, which is
// expected to fail evaluation.  The exact type of failure
// (assertion, parse error, etc) is not currently checked.
#[rstest]
fn eval_fail(#[files("src/tests/tvix_tests/eval-fail-*.nix")] code_path: PathBuf) {
    eval_test(code_path, false)
}

// eval-fail-* tests from the original Nix test suite.
#[cfg(feature = "nix_tests")]
#[rstest]
fn nix_eval_fail(#[files("src/tests/nix_tests/eval-fail-*.nix")] code_path: PathBuf) {
    eval_test(code_path, false)
}
