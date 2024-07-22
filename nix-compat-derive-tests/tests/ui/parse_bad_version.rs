use nix_compat_derive::NixDeserialize;

#[derive(NixDeserialize)]
pub struct Test {
    #[nix(version = 12)]
    version: u8,
}

fn main() {}
