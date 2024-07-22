use nix_compat_derive::NixDeserialize;

#[derive(NixDeserialize)]
pub struct Value(String);

#[derive(NixDeserialize)]
pub struct Test {
    #[nix(version = "20..")]
    version: Value,
}

fn main() {}
