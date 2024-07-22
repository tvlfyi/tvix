use nix_compat_derive::NixDeserialize;

#[derive(NixDeserialize)]
pub enum Test {
    #[nix(version = "..=10")]
    Old,
    #[nix(version = "15..=17")]
    Legacy,
    #[nix(version = "50..")]
    NewWay,
}

fn main() {}
