use crate::nixbase32;
use crate::nixhash::NixHash;
use crate::store_path::StorePath;
use sha2::{Digest, Sha256};

use super::Error;

/// compress_hash takes an arbitrarily long sequence of bytes (usually
/// a hash digest), and returns a sequence of bytes of length
/// OUTPUT_SIZE.
///
/// It's calculated by rotating through the bytes in the output buffer
/// (zero- initialized), and XOR'ing with each byte of the passed
/// input. It consumes 1 byte at a time, and XOR's it with the current
/// value in the output buffer.
///
/// This mimics equivalent functionality in C++ Nix.
pub fn compress_hash<const OUTPUT_SIZE: usize>(input: &[u8]) -> [u8; OUTPUT_SIZE] {
    let mut output = [0; OUTPUT_SIZE];

    for (ii, ch) in input.iter().enumerate() {
        output[ii % OUTPUT_SIZE] ^= ch;
    }

    output
}

/// This builds a store path, by calculating the text_hash_string of either a
/// derivation or a literal text file that may contain references.
pub fn build_store_path_from_references<
    S: AsRef<str>,
    I: IntoIterator<Item = S>,
    C: AsRef<[u8]>,
>(
    name: &str,
    content: C,
    references: I,
) -> Result<StorePath, Error> {
    let text_hash_str = text_hash_string(name, content, references);
    build_store_path_from_fingerprint(name, &text_hash_str)
}

/// This builds a store path from a fingerprint.
/// Usually, that function is used from [build_store_path_from_references] and
/// passed a "text hash string" (starting with "text:" as fingerprint),
/// but other fingerprints starting with "output:" are also used in Derivation
/// output path calculation.
///
/// The fingerprint is hashed with sha256, its digest is compressed to 20 bytes,
/// and nixbase32-encoded (32 characters).
pub fn build_store_path_from_fingerprint(
    name: &str,
    fingerprint: &str,
) -> Result<StorePath, Error> {
    let digest = {
        let hasher = Sha256::new_with_prefix(fingerprint);
        hasher.finalize()
    };
    let compressed = compress_hash::<20>(&digest);
    StorePath::validate_name(name)?;
    Ok(StorePath {
        digest: compressed,
        name: name.to_string(),
    })
}

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

    s.push_str(&format!(":{}:{}", crate::store_path::STORE_DIR, name));

    s
}

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

#[cfg(test)]
mod test {
    use crate::store_path::build_store_path_from_references;

    #[test]
    fn build_store_path_with_zero_references() {
        // This hash should match `builtins.toFile`, e.g.:
        //
        // nix-repl> builtins.toFile "foo" "bar"
        // "/nix/store/vxjiwkjkn7x4079qvh1jkl5pn05j2aw0-foo"

        let store_path = build_store_path_from_references("foo", "bar", Vec::<String>::new())
            .expect("build_store_path() should succeed");

        assert_eq!(
            store_path.to_absolute_path().as_str(),
            "/nix/store/vxjiwkjkn7x4079qvh1jkl5pn05j2aw0-foo"
        );
    }

    #[test]
    fn build_store_path_with_non_zero_references() {
        // This hash should match:
        //
        // nix-repl> builtins.toFile "baz" "${builtins.toFile "foo" "bar"}"
        // "/nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz"

        let inner = build_store_path_from_references("foo", "bar", Vec::<String>::new())
            .expect("path_with_references() should succeed");
        let inner_path = inner.to_absolute_path();

        let outer = build_store_path_from_references("baz", &inner_path, vec![inner_path.as_str()])
            .expect("path_with_references() should succeed");

        assert_eq!(
            outer.to_absolute_path().as_str(),
            "/nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz"
        );
    }
}
