use std::fmt::{self, Display};

use data_encoding::BASE64;
use ed25519_dalek::SIGNATURE_LENGTH;

#[derive(Debug)]
pub struct Signature<'a> {
    name: &'a str,
    bytes: [u8; SIGNATURE_LENGTH],
}

impl<'a> Signature<'a> {
    pub fn new(name: &'a str, bytes: [u8; SIGNATURE_LENGTH]) -> Self {
        Self { name, bytes }
    }

    pub fn parse(input: &'a str) -> Result<Self, Error> {
        let (name, bytes64) = input.split_once(':').ok_or(Error::MissingSeparator)?;

        if name.is_empty()
            || !name
                .chars()
                .all(|c| char::is_alphanumeric(c) || c == '-' || c == '.')
        {
            return Err(Error::InvalidName(name.to_string()));
        }

        if bytes64.len() != BASE64.encode_len(SIGNATURE_LENGTH) {
            return Err(Error::InvalidSignatureLen(bytes64.len()));
        }

        let mut bytes = [0; SIGNATURE_LENGTH];
        let mut buf = [0; SIGNATURE_LENGTH + 2];
        match BASE64.decode_mut(bytes64.as_bytes(), &mut buf) {
            Ok(SIGNATURE_LENGTH) => bytes.copy_from_slice(&buf[..SIGNATURE_LENGTH]),
            Ok(_) => unreachable!(),
            // keeping DecodePartial gets annoying lifetime-wise
            Err(_) => return Err(Error::DecodeError(input.to_string())),
        }

        Ok(Signature { name, bytes })
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn bytes(&self) -> &[u8; SIGNATURE_LENGTH] {
        &self.bytes
    }

    /// For a given fingerprint and ed25519 verifying key, ensure if the signature is valid.
    pub fn verify(&self, fingerprint: &[u8], verifying_key: &ed25519_dalek::VerifyingKey) -> bool {
        let signature = ed25519_dalek::Signature::from_bytes(self.bytes());

        verifying_key.verify_strict(fingerprint, &signature).is_ok()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid name: {0}")]
    InvalidName(String),
    #[error("Missing separator")]
    MissingSeparator,
    #[error("Invalid signature len: (expected {} b64-encoded, got {}", BASE64.encode_len(SIGNATURE_LENGTH), .0)]
    InvalidSignatureLen(usize),
    #[error("Unable to base64-decode signature: {0}")]
    DecodeError(String),
}

impl Display for Signature<'_> {
    fn fmt(&self, w: &mut fmt::Formatter) -> fmt::Result {
        write!(w, "{}:{}", self.name, BASE64.encode(&self.bytes))
    }
}

#[cfg(test)]
mod test {
    use data_encoding::BASE64;
    use ed25519_dalek::VerifyingKey;
    use lazy_static::lazy_static;

    use super::Signature;
    use test_case::test_case;

    const FINGERPRINT: &str = "1;/nix/store/syd87l2rxw8cbsxmxl853h0r6pdwhwjr-curl-7.82.0-bin;sha256:1b4sb93wp679q4zx9k1ignby1yna3z7c4c2ri3wphylbc2dwsys0;196040;/nix/store/0jqd0rlxzra1rs38rdxl43yh6rxchgc6-curl-7.82.0,/nix/store/6w8g7njm4mck5dmjxws0z1xnrxvl81xa-glibc-2.34-115,/nix/store/j5jxw3iy7bbz4a57fh9g2xm2gxmyal8h-zlib-1.2.12,/nix/store/yxvjs9drzsphm9pcf42a4byzj1kb9m7k-openssl-1.1.1n";

    // The signing key labelled as `cache.nixos.org-1`,
    lazy_static! {
        static ref PUB_CACHE_NIXOS_ORG_1: VerifyingKey = ed25519_dalek::VerifyingKey::from_bytes(
            BASE64
                .decode(b"6NCHdD59X431o0gWypbMrAURkbJ16ZPMQFGspcDShjY=")
                .unwrap()[..]
                .try_into()
                .unwrap()
        )
        .unwrap();
        static ref PUB_TEST_1: VerifyingKey = ed25519_dalek::VerifyingKey::from_bytes(
            BASE64
                .decode(b"tLAEn+EeaBUJYqEpTd2yeerr7Ic6+0vWe+aXL/vYUpE=")
                .unwrap()[..]
                .try_into()
                .unwrap()
        )
        .unwrap();
    }

    #[test_case(&PUB_CACHE_NIXOS_ORG_1, &"cache.nixos.org-1:TsTTb3WGTZKphvYdBHXwo6weVILmTytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==", FINGERPRINT, true; "valid cache.nixos.org-1")]
    #[test_case(&PUB_CACHE_NIXOS_ORG_1, &"cache.nixos.org-1:TsTTb3WGTZKphvYdBHXwo6weVILmTytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==", FINGERPRINT, true; "valid test1")]
    #[test_case(&PUB_CACHE_NIXOS_ORG_1, &"cache.nixos.org-2:TsTTb3WGTZKphvYdBHXwo6weVILmTytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==", FINGERPRINT, true; "valid cache.nixos.org different name")]
    #[test_case(&PUB_CACHE_NIXOS_ORG_1, &"cache.nixos.org-1:TsTTb000000000000000000000000ytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==", FINGERPRINT, false; "fail invalid cache.nixos.org-1 signature")]
    #[test_case(&PUB_CACHE_NIXOS_ORG_1, &"cache.nixos.org-1:TsTTb3WGTZKphvYdBHXwo6weVILmTytUjLB+vcX89fOjjRicCHmKA4RCPMVLkj6TMJ4GMX3HPVWRdD1hkeKZBQ==", &FINGERPRINT[0..5], false; "fail valid sig but wrong fp cache.nixos.org-1")]
    fn verify_sigs(
        verifying_key: &VerifyingKey,
        sig_str: &'static str,
        fp: &str,
        expect_valid: bool,
    ) {
        let sig = Signature::parse(sig_str).expect("must parse");
        assert_eq!(expect_valid, sig.verify(fp.as_bytes(), verifying_key));
    }

    #[test_case("cache.nixos.org-1:o1DTsjCz0PofLJ216P2RBuSulI8BAb6zHxWE4N+tzlcELk5Uk/GO2SCxWTRN5wJutLZZ+cHTMdWqOHF8"; "wrong length")]
    #[test_case("test\n:u01BybwQhyI5H1bW1EIWXssMDhDDIvXOG5uh8Qzgdyjz6U1qg6DHhMAvXZOUStIj6X5t4/ufFgR8i3fjf0bMAw=="; "wrong name newline")]
    #[test_case("test :u01BybwQhyI5H1bW1EIWXssMDhDDIvXOG5uh8Qzgdyjz6U1qg6DHhMAvXZOUStIj6X5t4/ufFgR8i3fjf0bMAw=="; "wrong name space")]
    #[test_case(":u01BybwQhyI5H1bW1EIWXssMDhDDIvXOG5uh8Qzgdyjz6U1qg6DHhMAvXZOUStIj6X5t4/ufFgR8i3fjf0bMAw=="; "empty name")]
    #[test_case("u01BybwQhyI5H1bW1EIWXssMDhDDIvXOG5uh8Qzgdyjz6U1qg6DHhMAvXZOUStIj6X5t4/ufFgR8i3fjf0bMAw=="; "b64 only")]
    fn parse_fail(input: &'static str) {
        Signature::parse(input).expect_err("must fail");
    }
}
