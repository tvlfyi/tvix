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
