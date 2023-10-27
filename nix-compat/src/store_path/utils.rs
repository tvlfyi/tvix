use crate::nixbase32;
use crate::nixhash::{CAHash, NixHash};
use crate::store_path::{Error, StorePath, STORE_DIR};
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
pub fn build_text_path<S: AsRef<str>, I: IntoIterator<Item = S>, C: AsRef<[u8]>>(
    name: &str,
    content: C,
    references: I,
) -> Result<StorePath, BuildStorePathError> {
    // produce the sha256 digest of the contents
    let content_digest = Sha256::new_with_prefix(content).finalize().into();

    build_ca_path(name, &CAHash::Text(content_digest), references, false)
}

/// This builds a store path from a [CAHash] and a list of references.
pub fn build_ca_path<B: AsRef<[u8]>, S: AsRef<str>, I: IntoIterator<Item = S>>(
    name: B,
    ca_hash: &CAHash,
    references: I,
    self_reference: bool,
) -> Result<StorePath, BuildStorePathError> {
    match &ca_hash {
        CAHash::Text(ref digest) => {
            if self_reference {
                return Err(BuildStorePathError::InvalidReference());
            }
            build_store_path_from_fingerprint_parts(
                &make_references_string("text", references, false),
                &NixHash::Sha256(*digest),
                name,
            )
            .map_err(BuildStorePathError::InvalidStorePath)
        }
        CAHash::Nar(ref hash @ NixHash::Sha256(_)) => build_store_path_from_fingerprint_parts(
            &make_references_string("source", references, self_reference),
            hash,
            name,
        )
        .map_err(BuildStorePathError::InvalidStorePath),
        // for all other CAHash::Nar, another custom scheme is used.
        CAHash::Nar(ref hash) => {
            if references.into_iter().next().is_some() {
                return Err(BuildStorePathError::InvalidReference());
            }
            if self_reference {
                return Err(BuildStorePathError::InvalidReference());
            }
            build_store_path_from_fingerprint_parts(
                "output:out",
                &{
                    NixHash::Sha256(
                        Sha256::new_with_prefix(format!(
                            "fixed:out:r:{}:",
                            hash.to_nix_hash_string()
                        ))
                        .finalize()
                        .into(),
                    )
                },
                name,
            )
            .map_err(BuildStorePathError::InvalidStorePath)
        }
        // CaHash::Flat is using something very similar, except the `r:` prefix.
        CAHash::Flat(ref hash) => {
            if references.into_iter().next().is_some() {
                return Err(BuildStorePathError::InvalidReference());
            }
            if self_reference {
                return Err(BuildStorePathError::InvalidReference());
            }
            build_store_path_from_fingerprint_parts(
                "output:out",
                &{
                    NixHash::Sha256(
                        Sha256::new_with_prefix(format!(
                            "fixed:out:{}:",
                            hash.to_nix_hash_string()
                        ))
                        .finalize()
                        .into(),
                    )
                },
                name,
            )
            .map_err(BuildStorePathError::InvalidStorePath)
        }
    }
}

/// For given NAR sha256 digest and name, return the new [StorePath] this would have.
pub fn build_nar_based_store_path(nar_sha256_digest: &[u8; 32], name: &str) -> StorePath {
    let nar_hash_with_mode = CAHash::Nar(NixHash::Sha256(nar_sha256_digest.to_owned()));

    build_ca_path(name, &nar_hash_with_mode, Vec::<String>::new(), false).unwrap()
}

/// This builds an input-addressed store path.
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
fn build_store_path_from_fingerprint_parts<B: AsRef<[u8]>>(
    ty: &str,
    hash: &NixHash,
    name: B,
) -> Result<StorePath, Error> {
    let name = super::validate_name(name.as_ref())?;
    let fingerprint =
        String::from(ty) + ":" + &hash.to_nix_hash_string() + ":" + STORE_DIR + ":" + &name;
    let digest = Sha256::new_with_prefix(fingerprint).finalize();
    let compressed = compress_hash::<20>(&digest);

    Ok(StorePath {
        digest: compressed,
        name,
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
    use super::*;
    use crate::nixhash::{CAHash, NixHash};

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
        let outer = build_ca_path(
            "bar",
            &CAHash::Nar(NixHash::Sha1(
                data_encoding::HEXLOWER
                    .decode(b"0beec7b5ea3f0fdbc95d0dd47f3c5bc275da8a33")
                    .expect("hex should decode")
                    .try_into()
                    .expect("should have right len"),
            )),
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
        let outer = build_ca_path(
            "baz",
            &CAHash::Nar(NixHash::Sha256(
                nixbase32::decode(b"1xqkzcb3909fp07qngljr4wcdnrh1gdam1m2n29i6hhrxlmkgkv1")
                    .expect("hex should decode")
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
