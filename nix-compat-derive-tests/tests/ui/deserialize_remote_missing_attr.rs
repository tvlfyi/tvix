use nix_compat_derive::nix_deserialize_remote;

pub struct Value(String);
impl From<String> for Value {
    fn from(s: String) -> Value {
        Value(s)
    }
}

nix_deserialize_remote!(
    #[nix()]
    Value
);

fn main() {}
