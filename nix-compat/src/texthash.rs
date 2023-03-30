use sha2::{Digest, Sha256};

use crate::{nixhash::NixHash, store_path};

/// This contains the Nix logic to create "text hash strings", which are used
/// in `builtins.toFile`, as well as in Derivation Path calculation.
///
/// A text hash is calculated by concatenating the following fields, separated by a `:`:
///
///  - text
///  - references, individually joined by `:`
///  - the nix_hash_string representation of the sha256 digest of some contents
///  - the value of `storeDir`
///  - the name
pub fn text_hash_string<S: AsRef<str>, I: IntoIterator<Item = S>, C: AsRef<[u8]>>(
    name: &str,
    content: C,
    references: I,
) -> String {
    let mut s = String::from("text:");

    for reference in references {
        s.push_str(reference.as_ref());
        s.push(':');
    }

    // the nix_hash_string representation of the sha256 digest of some contents
    s.push_str(
        &{
            let content_digest = {
                let hasher = Sha256::new_with_prefix(content);
                hasher.finalize()
            };
            NixHash::new(crate::nixhash::HashAlgo::Sha256, content_digest.to_vec())
        }
        .to_nix_hash_string(),
    );

    s.push_str(&format!(":{}:{}", store_path::STORE_DIR, name));

    s
}
