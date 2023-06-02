//! Tests which use upstream nix as an oracle to test evaluation against

use std::{env, path::PathBuf, process::Command};

use pretty_assertions::assert_eq;
use tempdir::TempDir;

fn nix_binary_path() -> PathBuf {
    env::var("NIX_INSTANTIATE_BINARY_PATH")
        .unwrap_or_else(|_| "nix-instantiate".to_owned())
        .into()
}

#[derive(Clone, Copy)]
enum Strictness {
    Lazy,
    Strict,
}

fn nix_eval(expr: &str, strictness: Strictness) -> String {
    let store_dir = TempDir::new("store-dir").unwrap();

    let mut args = match strictness {
        Strictness::Lazy => vec![],
        Strictness::Strict => vec!["--strict"],
    };
    args.extend_from_slice(&["--eval", "-E"]);

    let output = Command::new(nix_binary_path())
        .args(&args[..])
        .arg(format!("({expr})"))
        .env(
            "NIX_REMOTE",
            format!("local?root={}", store_dir.path().display()),
        )
        .output()
        .unwrap();
    if !output.status.success() {
        panic!(
            "nix eval {expr} failed!\n    stdout: {}\n    stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    String::from_utf8(output.stdout).unwrap()
}

/// Compare the evaluation of the given nix expression in nix (using the
/// `NIX_INSTANTIATE_BINARY_PATH` env var to resolve the `nix-instantiate` binary) and tvix, and
/// assert that the result is identical
#[track_caller]
fn compare_eval(expr: &str, strictness: Strictness) {
    let nix_result = nix_eval(expr, strictness);
    let mut eval = tvix_eval::Evaluation::new(expr, None);
    eval.strict = matches!(strictness, Strictness::Strict);
    eval.io_handle = Box::new(tvix_eval::StdIO);

    let tvix_result = eval
        .evaluate()
        .value
        .expect("tvix evaluation should succeed")
        .to_string();

    assert_eq!(nix_result.trim(), tvix_result);
}

/// Generate a suite of tests which call [`compare_eval`] on expressions, checking that nix and tvix
/// return identical results.
macro_rules! compare_eval_tests {
    ($strictness:expr, {}) => {};
    ($strictness:expr, {$(#[$meta:meta])* $test_name: ident($expr: expr); $($rest:tt)*}) => {
        #[test]
        $(#[$meta])*
        fn $test_name() {
            compare_eval($expr, $strictness);
        }

        compare_eval_tests!($strictness, { $($rest)* });
    }
}

macro_rules! compare_strict_eval_tests {
    ($($tests:tt)*) => {
        compare_eval_tests!(Strictness::Lazy, { $($tests)* });
    }
}

macro_rules! compare_lazy_eval_tests {
    ($($tests:tt)*) => {
        compare_eval_tests!(Strictness::Lazy, { $($tests)* });
    }
}

compare_strict_eval_tests! {
    literal_int("1");
    add_ints("1 + 1");
    add_lists("[1 2] ++ [3 4]");
    add_paths(r#"[
        (./. + "/")
        (./foo + "bar")
        (let name = "bar"; in ./foo + name)
        (let name = "bar"; in ./foo + "${name}")
        (let name = "bar"; in ./foo + "/" + "${name}")
        (let name = "bar"; in ./foo + "/${name}")
        (./. + ./.)
    ]"#);
}

// TODO(sterni): tvix_tests should gain support for something similar in the future,
// but this requires messing with the path naming which would break compat with
// C++ Nix's test suite
compare_lazy_eval_tests! {
    // Wrap every expression type supported by [Compiler::compile] in a list
    // with lazy evaluation enabled, so we can check it being thunked or not
    // against C++ Nix.
    unthunked_literals_in_list("[ https://tvl.fyi 1 1.2 ]");
    unthunked_path_in_list("[ ./nix_oracle.rs ]");
    unthunked_string_literal_in_list("[ \":thonking:\" ]");
    thunked_unary_ops_in_list("[ (!true) (-1) ]");
    thunked_bin_ops_in_list(r#"
      let
        # Necessary to fool the optimiser for && and ||
        true' = true;
        false' = false;
      in
      [
        (true' && false')
        (true' || false')
        (false -> true)
        (40 + 2)
        (43 - 1)
        (21 * 2)
        (126 / 3)
        ({ } // { bar = null; })
        (12 == 13)
        (3 < 2)
        (4 > 2)
        (23 >= 42)
        (33 <= 22)
        ([ ] ++ [ ])
        (42 != null)
      ]
    "#);
    thunked_has_attrs_in_list("[ ({ } ? foo) ]");
    thunked_list_in_list("[ [ 1 2 3 ] ]");
    thunked_attr_set_in_list("[ { foo = null; } ]");
    thunked_select_in_list("[ ({ foo = null; }.bar) ]");
    thunked_assert_in_list("[ (assert false; 12) ]");
    thunked_if_in_list("[ (if false then 13 else 12) ]");
    thunked_let_in_list("[ (let foo = 12; in foo) ]");
    thunked_with_in_list("[ (with { foo = 13; }; fooo) ]");
    unthunked_identifier_in_list("let foo = 12; in [ foo ]");
    thunked_lambda_in_list("[ (x: x) ]");
    thunked_function_application_in_list("[ (builtins.add 1 2) ]");
    thunked_legacy_let_in_list("[ (let { foo = 12; body = foo; }) ]");

    unthunked_formals_fallback_literal("({ foo ? 12 }: [ foo ]) { }");
    unthunked_formals_fallback_string_literal("({ foo ? \"wiggly\" }: [ foo ]) { }");
    thunked_formals_fallback_application("({ foo ? builtins.add 1 2 }: [ foo ]) { }");
    thunked_formals_fallback_name_resolution_literal("({ foo ? bar, bar ? 12 }: [ foo ]) { }");
}
