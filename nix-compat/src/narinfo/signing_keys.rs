//! This module provides tooling to parse private key (pairs) produced by Nix
//! and its
//! `nix-store --generate-binary-cache-key name path.secret path.pub` command.
//! It produces `ed25519_dalek` keys, but the `NarInfo::add_signature` function
//! is generic, allowing other signers.

use data_encoding::BASE64;
use ed25519_dalek::{PUBLIC_KEY_LENGTH, SECRET_KEY_LENGTH};

use super::{SignatureRef, VerifyingKey};

pub struct SigningKey<S> {
    name: String,
    signing_key: S,
}

impl<S> SigningKey<S>
where
    S: ed25519::signature::Signer<ed25519::Signature>,
{
    /// Constructs a singing key, using a name and a signing key.
    pub fn new(name: String, signing_key: S) -> Self {
        Self { name, signing_key }
    }

    /// Signs a fingerprint using the internal signing key, returns the [SignatureRef]
    pub(crate) fn sign<'a>(&'a self, fp: &[u8]) -> SignatureRef<'a> {
        SignatureRef::new(&self.name, self.signing_key.sign(fp).to_bytes())
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Parses a SigningKey / VerifyingKey from a byte slice in the format that Nix uses.
pub fn parse_keypair(
    input: &str,
) -> Result<(SigningKey<ed25519_dalek::SigningKey>, VerifyingKey), Error> {
    let (name, bytes64) = input.split_once(':').ok_or(Error::MissingSeparator)?;

    if name.is_empty()
        || !name
            .chars()
            .all(|c| char::is_alphanumeric(c) || c == '-' || c == '.')
    {
        return Err(Error::InvalidName(name.to_string()));
    }

    const DECODED_BYTES_LEN: usize = SECRET_KEY_LENGTH + PUBLIC_KEY_LENGTH;
    if bytes64.len() != BASE64.encode_len(DECODED_BYTES_LEN) {
        return Err(Error::InvalidSigningKeyLen(bytes64.len()));
    }

    let mut buf = [0; DECODED_BYTES_LEN + 2]; // 64 bytes + 2 bytes padding
    let mut bytes = [0; DECODED_BYTES_LEN];
    match BASE64.decode_mut(bytes64.as_bytes(), &mut buf) {
        Ok(len) if len == DECODED_BYTES_LEN => {
            bytes.copy_from_slice(&buf[..DECODED_BYTES_LEN]);
        }
        Ok(_) => unreachable!(),
        // keeping DecodePartial gets annoying lifetime-wise
        Err(_) => return Err(Error::DecodeError(input.to_string())),
    }

    let bytes_signing_key: [u8; SECRET_KEY_LENGTH] = {
        let mut b = [0u8; SECRET_KEY_LENGTH];
        b.copy_from_slice(&bytes[0..SECRET_KEY_LENGTH]);
        b
    };
    let bytes_verifying_key: [u8; PUBLIC_KEY_LENGTH] = {
        let mut b = [0u8; PUBLIC_KEY_LENGTH];
        b.copy_from_slice(&bytes[SECRET_KEY_LENGTH..]);
        b
    };

    let signing_key = SigningKey::new(
        name.to_string(),
        ed25519_dalek::SigningKey::from_bytes(&bytes_signing_key),
    );

    let verifying_key = VerifyingKey::new(
        name.to_string(),
        ed25519_dalek::VerifyingKey::from_bytes(&bytes_verifying_key)
            .map_err(Error::InvalidVerifyingKey)?,
    );

    Ok((signing_key, verifying_key))
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid name: {0}")]
    InvalidName(String),
    #[error("Missing separator")]
    MissingSeparator,
    #[error("Invalid signing key len: {0}")]
    InvalidSigningKeyLen(usize),
    #[error("Unable to base64-decode signing key: {0}")]
    DecodeError(String),
    #[error("VerifyingKey error: {0}")]
    InvalidVerifyingKey(ed25519_dalek::SignatureError),
}

#[cfg(test)]
mod test {
    use crate::narinfo::DUMMY_KEYPAIR;
    #[test]
    fn parse() {
        let (_signing_key, _verifying_key) =
            super::parse_keypair(DUMMY_KEYPAIR).expect("must succeed");
    }

    #[test]
    fn parse_fail() {
        assert!(super::parse_keypair("cache.example.com-1:cCta2MEsRNuYCgWYyeRXLyfoFpKhQJKn8gLMeXWAb7vIpRKKo/3JoxJ24OYa3DxT2JVV38KjK/1ywHWuMe2JE").is_err());
        assert!(super::parse_keypair("cache.example.com-1cCta2MEsRNuYCgWYyeRXLyfoFpKhQJKn8gLMeXWAb7vIpRKKo/3JoxJ24OYa3DxT2JVV38KjK/1ywHWuMe2JE").is_err());
    }
}
