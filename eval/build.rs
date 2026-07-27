use std::env;

fn main() {
    println!(
        "cargo:rustc-env=TVIX_CURRENT_SYSTEM={}",
        env::var("TARGET").unwrap()
    );
    println!("cargo:rerun-if-changed-env=TARGET");

    // Pick up new test case files
    // https://github.com/la10736/rstest/issues/256
    println!("cargo:rerun-if-changed=src/tests/nix_tests");
    println!("cargo:rerun-if-changed=src/tests/tvix_tests")
}
