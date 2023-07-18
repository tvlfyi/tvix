use super::{Error, STORE_DIR};
use crate::nixbase32;
use crate::nixhash::{HashAlgo, NixHash, NixHashWithMode};
use crate::store_path::StorePath;
use sha2::{Digest, Sha256};
use thiserror;

/// Errors that can occur when creating a content-addressed store path.
///
/// This wraps the main [Error]..
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum BuildStorePathError {
    #[error("Invalid Store Path: {0}")]
    InvalidStorePath(Error),
    /// This error occurs when we have references outside the SHA-256 +
    /// Recursive case. The restriction comes from upstream Nix. It may be
    /// lifted at some point but there isn't a pressing need to anticipate that.
    #[error("References were not supported as much as requested")]
    InvalidReference(),
}

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
pub fn build_text_path<S: AsRef<str>, I: IntoIterator<Item = S>, C: AsRef<[u8]>>(
    name: &str,
    content: C,
    references: I,
) -> Result<StorePath, Error> {
    build_store_path_from_fingerprint_parts(
        &make_type("text", references, false),
        // the nix_hash_string representation of the sha256 digest of some contents
        &{
            let content_digest = {
                let hasher = Sha256::new_with_prefix(content);
                hasher.finalize()
            };
            NixHash::new(crate::nixhash::HashAlgo::Sha256, content_digest.to_vec())
        },
        name,
    )
}

/// This builds a more "regular" content-addressed store path
pub fn build_regular_ca_path<S: AsRef<str>, I: IntoIterator<Item = S>>(
    name: &str,
    hash_with_mode: &NixHashWithMode,
    references: I,
    self_reference: bool,
) -> Result<StorePath, BuildStorePathError> {
    match &hash_with_mode {
        NixHashWithMode::Recursive(
            ref hash @ NixHash {
                algo: HashAlgo::Sha256,
                ..
            },
        ) => build_store_path_from_fingerprint_parts(
            &make_type("source", references, self_reference),
            hash,
            name,
        )
        .map_err(BuildStorePathError::InvalidStorePath),
        _ => {
            if references.into_iter().next().is_some() {
                return Err(BuildStorePathError::InvalidReference());
            }
            if self_reference {
                return Err(BuildStorePathError::InvalidReference());
            }
            build_store_path_from_fingerprint_parts(
                "output:out",
                &{
                    let content_digest = {
                        let mut hasher = Sha256::new_with_prefix("fixed:out:");
                        hasher.update(hash_with_mode.mode().prefix());
                        hasher.update(hash_with_mode.digest().algo.to_string());
                        hasher.update(":");
                        hasher.update(
                            &data_encoding::HEXLOWER.encode(&hash_with_mode.digest().digest),
                        );
                        hasher.update(":");
                        hasher.finalize()
                    };
                    NixHash::new(crate::nixhash::HashAlgo::Sha256, content_digest.to_vec())
                },
                name,
            )
            .map_err(BuildStorePathError::InvalidStorePath)
        }
    }
}

/// This builds an input-addressed store path
///
/// Input-addresed store paths are always derivation outputs, the "input" in question is the
/// derivation and its closure.
pub fn build_output_path(
    drv_hash: &NixHash,
    output_name: &str,
    output_path_name: &str,
) -> Result<StorePath, Error> {
    build_store_path_from_fingerprint_parts(
        &(String::from("output:") + output_name),
        drv_hash,
        output_path_name,
    )
}

/// This builds a store path from fingerprint parts.
/// Usually, that function is used from [build_text_path] and
/// passed a "text hash string" (starting with "text:" as fingerprint),
/// but other fingerprints starting with "output:" are also used in Derivation
/// output path calculation.
///
/// The fingerprint is hashed with sha256, its digest is compressed to 20 bytes,
/// and nixbase32-encoded (32 characters).
fn build_store_path_from_fingerprint_parts(
    ty: &str,
    hash: &NixHash,
    name: &str,
) -> Result<StorePath, Error> {
    let fingerprint =
        String::from(ty) + ":" + &hash.to_nix_hash_string() + ":" + STORE_DIR + ":" + name;
    let digest = {
        let hasher = Sha256::new_with_prefix(fingerprint);
        hasher.finalize()
    };
    let compressed = compress_hash::<20>(&digest);
    super::validate_name(name.as_bytes())?;
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
fn make_type<S: AsRef<str>, I: IntoIterator<Item = S>>(
    ty: &str,
    references: I,
    self_ref: bool,
) -> String {
    let mut s = String::from(ty);

    for reference in references {
        s.push(':');
        s.push_str(reference.as_ref());
    }

    if self_ref {
        s.push_str(":self");
    }

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
    use super::*;
    use crate::nixhash::{NixHash, NixHashWithMode};

    #[test]
    fn build_text_path_with_zero_references() {
        // This hash should match `builtins.toFile`, e.g.:
        //
        // nix-repl> builtins.toFile "foo" "bar"
        // "/nix/store/vxjiwkjkn7x4079qvh1jkl5pn05j2aw0-foo"

        let store_path = build_text_path("foo", "bar", Vec::<String>::new())
            .expect("build_store_path() should succeed");

        assert_eq!(
            store_path.to_absolute_path().as_str(),
            "/nix/store/vxjiwkjkn7x4079qvh1jkl5pn05j2aw0-foo"
        );
    }

    #[test]
    fn build_text_path_with_non_zero_references() {
        // This hash should match:
        //
        // nix-repl> builtins.toFile "baz" "${builtins.toFile "foo" "bar"}"
        // "/nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz"

        let inner = build_text_path("foo", "bar", Vec::<String>::new())
            .expect("path_with_references() should succeed");
        let inner_path = inner.to_absolute_path();

        let outer = build_text_path("baz", &inner_path, vec![inner_path.as_str()])
            .expect("path_with_references() should succeed");

        assert_eq!(
            outer.to_absolute_path().as_str(),
            "/nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz"
        );
    }

    #[test]
    fn build_sha1_path() {
        let outer = build_regular_ca_path(
            "bar",
            &NixHashWithMode::Recursive(NixHash {
                algo: HashAlgo::Sha1,
                digest: data_encoding::HEXLOWER
                    .decode(b"0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33")
                    .expect("hex should decode"),
            }),
            Vec::<String>::new(),
            false,
        )
        .expect("path_with_references() should succeed");

        assert_eq!(
            outer.to_absolute_path().as_str(),
            "/nix/store/mp57d33657rf34lzvlbpfa1gjfv5gmpg-bar"
        );
    }

    #[test]
    fn build_store_path_with_non_zero_references() {
        // This hash should match:
        //
        // nix-repl> builtins.toFile "baz" "${builtins.toFile "foo" "bar"}"
        // "/nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz"
        //
        // $ nix store make-content-addressed /nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz
        // rewrote '/nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz' to '/nix/store/s89y431zzhmdn3k8r96rvakryddkpv2v-baz'
        let outer = build_regular_ca_path(
            "baz",
            &NixHashWithMode::Recursive(NixHash {
                algo: HashAlgo::Sha256,
                digest: nixbase32::decode(b"1xqkzcb3909fp07qngljr4wcdnrh1gdam1m2n29i6hhrxlmkgkv1")
                    .expect("hex should decode"),
            }),
            vec!["/nix/store/dxwkwjzdaq7ka55pkk252gh32bgpmql4-foo"],
            false,
        )
        .expect("path_with_references() should succeed");

        assert_eq!(
            outer.to_absolute_path().as_str(),
            "/nix/store/s89y431zzhmdn3k8r96rvakryddkpv2v-baz"
        );
    }
}
