use sha2::{Digest, Sha256};

pub mod derivation;
pub mod nar;
pub mod nixbase32;
pub mod store_path;

/// Nix placeholders (i.e. values returned by `builtins.placeholder`)
/// are used to populate outputs with paths that must be
/// string-replaced with the actual placeholders later, at runtime.
///
/// The actual placeholder is basically just a SHA256 hash encoded in
/// cppnix format.
pub fn hash_placeholder(name: &str) -> String {
    let digest = {
        let mut hasher = Sha256::new();
        hasher.update(format!("nix-output:{}", name));
        hasher.finalize()
    };

    format!("/{}", nixbase32::encode(&digest))
}
