fn main() {
    // Pick up new test case files
    // https://github.com/la10736/rstest/issues/256
    println!("cargo:rerun-if-changed=src/derivation/tests/derivation_tests")
}
