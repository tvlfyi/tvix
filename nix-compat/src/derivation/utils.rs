use crate::derivation::DerivationError;
use crate::nixbase32;
use crate::store_path::StorePath;
use crate::texthash::text_hash_string;
use sha2::{Digest, Sha256};

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
fn compress_hash(input: &[u8], output_size: usize) -> Vec<u8> {
    let mut output: Vec<u8> = vec![0; output_size];

    for (ii, ch) in input.iter().enumerate() {
        output[ii % output_size] ^= ch;
    }

    output
}

/// This returns a store path, either of a derivation or a regular output.
/// The string is hashed with sha256, its digest is compressed to 20 bytes, and
/// nixbase32-encoded (32 characters)
pub(super) fn build_store_path(
    fingerprint: &str,
    name: &str,
) -> Result<StorePath, DerivationError> {
    let digest = {
        let hasher = Sha256::new_with_prefix(fingerprint);
        hasher.finalize()
    };
    let compressed = compress_hash(&digest, 20);
    StorePath::from_string(format!("{}-{}", nixbase32::encode(&compressed), name,).as_str())
        .map_err(|_e| DerivationError::InvalidOutputName(name.to_string()))
    // Constructing the StorePath can only fail if the passed output name was
    // invalid, so map errors to a [DerivationError::InvalidOutputName].
}

/// Build a store path for a literal text file in the store that may
/// contain references.
pub fn path_with_references<S: AsRef<str>, I: IntoIterator<Item = S>, C: AsRef<[u8]>>(
    name: &str,
    content: C,
    references: I,
) -> Result<StorePath, DerivationError> {
    let text_hash_str = text_hash_string(name, content, references);
    build_store_path(&text_hash_str, name)
}
