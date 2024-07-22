use nix_compat_derive::NixDeserialize;

#[derive(NixDeserialize)]
pub struct Test {
    #[nix]
    version: u8,
}

fn main() {}
