use crate::nixbase32;
use crate::store_path::StorePath;
use crate::texthash::text_hash_string;
use sha2::{Digest, Sha256};

use super::Error;

/// compress_hash takes an arbitrarily long sequence of bytes (usually
/// a hash digest), and returns a sequence of bytes of length
/// output_size.
///
/// It's calculated by rotating through the bytes in the output buffer
/// (zero- initialized), and XOR'ing with each byte of the passed
/// input. It consumes 1 byte at a time, and XOR's it with the current
/// value in the output buffer.
///
/// This mimics equivalent functionality in C++ Nix.
pub fn compress_hash(input: &[u8], output_size: usize) -> Vec<u8> {
    let mut output: Vec<u8> = vec![0; output_size];

    for (ii, ch) in input.iter().enumerate() {
        output[ii % output_size] ^= ch;
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
    let compressed = compress_hash(&digest, 20);
    StorePath::from_string(format!("{}-{}", nixbase32::encode(&compressed), name).as_str())
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
