use nix_compat_derive::NixDeserialize;

pub struct BadType;

#[derive(NixDeserialize)]
pub struct Test {
    version: BadType,
}

fn main() {}
