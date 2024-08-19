//! This module defines data structures and parsers for the public key format
//! used inside Nix to verify signatures on .narinfo files.

use std::fmt::Display;

use data_encoding::BASE64;
use ed25519_dalek::PUBLIC_KEY_LENGTH;

use super::SignatureRef;

/// This represents a ed25519 public key and "name".
/// These are normally passed in the `trusted-public-keys` Nix config option,
/// and consist of a name and base64-encoded ed25519 pubkey, separated by a `:`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyingKey {
    name: String,
    verifying_key: ed25519_dalek::VerifyingKey,
}

impl VerifyingKey {
    pub fn new(name: String, verifying_key: ed25519_dalek::VerifyingKey) -> Self {
        Self {
            name,
            verifying_key,
        }
    }

    pub fn parse(input: &str) -> Result<Self, Error> {
        let (name, bytes64) = input.split_once(':').ok_or(Error::MissingSeparator)?;

        if name.is_empty()
            || !name
                .chars()
                .all(|c| char::is_alphanumeric(c) || c == '-' || c == '.')
        {
            return Err(Error::InvalidName(name.to_string()));
        }

        if bytes64.len() != BASE64.encode_len(PUBLIC_KEY_LENGTH) {
            return Err(Error::InvalidVerifyingKeyLen(bytes64.len()));
        }

        let mut buf = [0; PUBLIC_KEY_LENGTH + 1];
        let mut bytes = [0; PUBLIC_KEY_LENGTH];
        match BASE64.decode_mut(bytes64.as_bytes(), &mut buf) {
            Ok(PUBLIC_KEY_LENGTH) => {
                bytes.copy_from_slice(&buf[..PUBLIC_KEY_LENGTH]);
            }
            Ok(_) => unreachable!(),
            // keeping DecodePartial gets annoying lifetime-wise
            Err(_) => return Err(Error::DecodeError(input.to_string())),
        }

        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(Error::InvalidVerifyingKey)?;

        Ok(Self {
            name: name.to_string(),
            verifying_key,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Verify the passed in signature is a correct signature for the passed in fingerprint and is signed
    /// by the key material referred to by [Self],
    /// which means the name in the signature has to match,
    /// and the signature bytes themselves need to be a valid signature made by
    /// the signing key identified by [Self::verifying key].
    pub fn verify(&self, fingerprint: &str, signature: &SignatureRef<'_>) -> bool {
        if self.name() != *signature.name() {
            return false;
        }

        return signature.verify(fingerprint.as_bytes(), &self.verifying_key);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid name: {0}")]
    InvalidName(String),
    #[error("Missing separator")]
    MissingSeparator,
    #[error("Invalid pubkey len: {0}")]
    InvalidVerifyingKeyLen(usize),
    #[error("VerifyingKey error: {0}")]
    InvalidVerifyingKey(ed25519_dalek::SignatureError),
    #[error("Unable to base64-decode pubkey: {0}")]
    DecodeError(String),
}

impl Display for VerifyingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.name,
            BASE64.encode(self.verifying_key.as_bytes())
        )
    }
}

#[cfg(test)]
mod test {
    use data_encoding::BASE64;
    use ed25519_dalek::PUBLIC_KEY_LENGTH;
    use rstest::rstest;

    use crate::narinfo::SignatureRef;

    use super::VerifyingKey;
    const FINGERPRINT: &str = "1;/nix/store/syd87l2rxw8cbsxmxl853h0r6pdwhwjr-curl-7.82.0-bin;sha256:1b4sb93wp679q4zx9k1ignby1yna3z7c4c2ri3wphylbc2dwsys0;196040;/nix/store/0jqd0rlxzra1rs38rdxl43yh6rxchgc6-curl-7.82.0,/nix/store/6w8g7njm4mck5dmjxws0z1xnrxvl81xa-glibc-2.34-115,/nix/store/j5jxw3iy7bbz4a57fh9g2xm2gxmyal8h-zlib-1.2.12,/nix/store/yxvjs9drzsphm9pcf42a4byzj1kb9m7k-openssl-1.1.1n";

    #[rstest]
    #[case::cache_nixos_org("cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=", "cache.nixos.org-1", &BASE64.decode(b"6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=").unwrap()[..].try_into().unwrap())]
    #[case::cache_nixos_org_different_name("cheesecake:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=", "cheesecake", &BASE64.decode(b"6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=").unwrap()[..].try_into().unwrap())]
    #[case::test_1("test1:tLAEn+EeaBUJYqEpTd2yeerr7Ic6+0vWe+aXL/vYUpE=", "test1", &BASE64.decode(b"tLAEn+EeaBUJYqEpTd2yeerr7Ic6+0vWe+aXL/vYUpE=").unwrap()[..].try_into().unwrap())]
    fn parse(
        #[case] input: &'static str,
        #[case] exp_name: &'static str,
        #[case] exp_verifying_key_bytes: &[u8; PUBLIC_KEY_LENGTH],
    ) {
        let pubkey = VerifyingKey::parse(input).expect("must parse");
        assert_eq!(exp_name, pubkey.name());
        assert_eq!(exp_verifying_key_bytes, pubkey.verifying_key.as_bytes());
    }

    #[rstest]
    #[case::empty_name("6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=")]
    #[case::missing_padding("cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY")]
    #[case::wrong_length("cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDS")]
    fn parse_fail(#[case] input: &'static str) {
        VerifyingKey::parse(input).expect_err("must fail");
    }

    #[rstest]
    #[case::correct_cache_nixos_org("cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=", FINGERPRINT, "cache.nixos.org-1:TsTTb3WGTZKphvYdBHXwo6weVILmTytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==", true)]
    #[case::wrong_name_mismatch("cache.nixos.org-1:6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=", FINGERPRINT, "cache.nixos.org:TsTTb3WGTZKphvYdBHXwo6weVILmTytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==", false)]
    fn verify(
        #[case] pubkey_str: &'static str,
        #[case] fingerprint: &'static str,
        #[case] signature_str: &'static str,
        #[case] expected: bool,
    ) {
        let pubkey = VerifyingKey::parse(pubkey_str).expect("must parse");
        let signature = SignatureRef::parse(signature_str).expect("must parse");

        assert_eq!(expected, pubkey.verify(fingerprint, &signature));
    }
}
