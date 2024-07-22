use std::str::FromStr;

use nix_compat_derive::NixDeserialize;

#[derive(NixDeserialize)]
#[nix(from_str)]
pub struct Test;

impl FromStr for Test {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "test" {
            Ok(Test)
        } else {
            Err(())
        }
    }
}

fn main() {}
