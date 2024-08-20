use crate::nixbase32;
use crate::nixhash::{CAHash, NixHash};
use crate::store_path::{Error, StorePath, StorePathRef, STORE_DIR};
use data_encoding::HEXLOWER;
use sha2::{Digest, Sha256};
use thiserror;

/// Errors that can occur when creating a content-addressed store path.
///
/// This wraps the main [crate::store_path::Error]..
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
/// If you don't want to have to pass the entire contents, you might want to use
/// [build_ca_path] instead.
pub fn build_text_path<'a, S, SP, I, C>(
    name: &'a str,
    content: C,
    references: I,
) -> Result<StorePath<SP>, BuildStorePathError>
where
    S: AsRef<str>,
    SP: std::cmp::Eq + std::ops::Deref<Target = str> + std::convert::From<&'a str>,
    I: IntoIterator<Item = S>,
    C: AsRef<[u8]>,
{
    // produce the sha256 digest of the contents
    let content_digest = Sha256::new_with_prefix(content).finalize().into();

    build_ca_path(name, &CAHash::Text(content_digest), references, false)
}

/// This builds a store path from a [CAHash] and a list of references.
pub fn build_ca_path<'a, S, SP, I>(
    name: &'a str,
    ca_hash: &CAHash,
    references: I,
    self_reference: bool,
) -> Result<StorePath<SP>, BuildStorePathError>
where
    S: AsRef<str>,
    SP: std::cmp::Eq + std::ops::Deref<Target = str> + std::convert::From<&'a str>,
    I: IntoIterator<Item = S>,
{
    // self references are only allowed for CAHash::Nar(NixHash::Sha256(_)).
    if self_reference && matches!(ca_hash, CAHash::Nar(NixHash::Sha256(_))) {
        return Err(BuildStorePathError::InvalidReference());
    }

    /// Helper function, used for the non-sha256 [CAHash::Nar] and all [CAHash::Flat].
    fn fixed_out_digest(prefix: &str, hash: &NixHash) -> [u8; 32] {
        Sha256::new_with_prefix(format!("{}:{}:", prefix, hash.to_nix_hex_string()))
            .finalize()
            .into()
    }

    let (ty, inner_digest) = match &ca_hash {
        CAHash::Text(ref digest) => (make_references_string("text", references, false), *digest),
        CAHash::Nar(NixHash::Sha256(ref digest)) => (
            make_references_string("source", references, self_reference),
            *digest,
        ),

        // for all other CAHash::Nar, another custom scheme is used.
        CAHash::Nar(ref hash) => {
            if references.into_iter().next().is_some() {
                return Err(BuildStorePathError::InvalidReference());
            }

            (
                "output:out".to_string(),
                fixed_out_digest("fixed:out:r", hash),
            )
        }
        // CaHash::Flat is using something very similar, except the `r:` prefix.
        CAHash::Flat(ref hash) => {
            if references.into_iter().next().is_some() {
                return Err(BuildStorePathError::InvalidReference());
            }

            (
                "output:out".to_string(),
                fixed_out_digest("fixed:out", hash),
            )
        }
    };

    build_store_path_from_fingerprint_parts(&ty, &inner_digest, name)
        .map_err(BuildStorePathError::InvalidStorePath)
}

/// For given NAR sha256 digest and name, return the new [StorePathRef] this
/// would have, or an error, in case the name is invalid.
pub fn build_nar_based_store_path<'a>(
    nar_sha256_digest: &[u8; 32],
    name: &'a str,
) -> Result<StorePathRef<'a>, BuildStorePathError> {
    let nar_hash_with_mode = CAHash::Nar(NixHash::Sha256(nar_sha256_digest.to_owned()));

    build_ca_path(name, &nar_hash_with_mode, Vec::<String>::new(), false)
}

/// This builds an input-addressed store path.
///
/// Input-addresed store paths are always derivation outputs, the "input" in question is the
/// derivation and its closure.
pub fn build_output_path<'a>(
    drv_sha256: &[u8; 32],
    output_name: &str,
    output_path_name: &'a str,
) -> Result<StorePathRef<'a>, Error> {
    build_store_path_from_fingerprint_parts(
        &(String::from("output:") + output_name),
        drv_sha256,
        output_path_name,
    )
}

/// This builds a store path from fingerprint parts.
/// Usually, that function is used from [build_text_path] and
/// passed a "text hash string" (starting with "text:" as fingerprint),
/// but other fingerprints starting with "output:" are also used in Derivation
/// output path calculation.
///
/// The fingerprint is hashed with sha256, and its digest is compressed to 20
/// bytes.
/// Inside a StorePath, that digest is printed nixbase32-encoded
/// (32 characters).
fn build_store_path_from_fingerprint_parts<'a, S>(
    ty: &str,
    inner_digest: &[u8; 32],
    name: &'a str,
) -> Result<StorePath<S>, Error>
where
    S: std::cmp::Eq + std::ops::Deref<Target = str> + std::convert::From<&'a str>,
{
    let fingerprint = format!(
        "{ty}:sha256:{}:{STORE_DIR}:{name}",
        HEXLOWER.encode(inner_digest)
    );
    // name validation happens in here.
    StorePath::from_name_and_digest_fixed(
        name,
        compress_hash(&Sha256::new_with_prefix(fingerprint).finalize()),
    )
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
fn make_references_string<S: AsRef<str>, I: IntoIterator<Item = S>>(
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
    let digest = Sha256::new_with_prefix(format!("nix-output:{}", name)).finalize();

    format!("/{}", nixbase32::encode(&digest))
}

#[cfg(test)]
mod test {
    use hex_literal::hex;

    use super::*;
    use crate::nixhash::{CAHash, NixHash};

    #[test]
    fn build_text_path_with_zero_references() {
        // This hash should match `builtins.toFile`, e.g.:
        //
        // nix-repl> builtins.toFile "foo" "bar"
        // "/nix/store/vxjiwkjkn7x4079qvh1jkl5pn05j2aw0-foo"

        let store_path: StorePathRef = build_text_path("foo", "bar", Vec::<String>::new())
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

        let inner: StorePathRef = build_text_path("foo", "bar", Vec::<String>::new())
            .expect("path_with_references() should succeed");
        let inner_path = inner.to_absolute_path();

        let outer: StorePathRef = build_text_path("baz", &inner_path, vec![inner_path.as_str()])
            .expect("path_with_references() should succeed");

        assert_eq!(
            outer.to_absolute_path().as_str(),
            "/nix/store/5xd714cbfnkz02h2vbsj4fm03x3f15nf-baz"
        );
    }

    #[test]
    fn build_sha1_path() {
        let outer: StorePathRef = build_ca_path(
            "bar",
            &CAHash::Nar(NixHash::Sha1(hex!(
                "0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33"
            ))),
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
        let outer: StorePathRef = build_ca_path(
            "baz",
            &CAHash::Nar(NixHash::Sha256(
                nixbase32::decode(b"1xqkzcb3909fp07qngljr4wcdnrh1gdam1m2n29i6hhrxlmkgkv1")
                    .expect("nixbase32 should decode")
                    .try_into()
                    .expect("should have right len"),
            )),
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
