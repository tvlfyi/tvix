use std::env;

fn main() {
    println!(
        "cargo:rustc-env=TVIX_CURRENT_SYSTEM={}",
        &env::var("TARGET").unwrap()
    );
    println!("cargo:rerun-if-changed-env=TARGET")
}
