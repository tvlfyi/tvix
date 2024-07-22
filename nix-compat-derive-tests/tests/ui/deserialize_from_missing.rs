use nix_compat_derive::NixDeserialize;

#[derive(NixDeserialize)]
#[nix(from = "u64")]
pub struct Test;

fn main() {}
