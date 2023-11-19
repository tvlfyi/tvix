use std::fmt::{self, Display};

use data_encoding::BASE64;

#[derive(Debug)]
pub struct Signature<'a> {
    name: &'a str,
    bytes: [u8; 64],
}

impl<'a> Signature<'a> {
    pub fn parse(input: &'a str) -> Result<Signature<'a>, SignatureError> {
        let (name, bytes64) = input
            .split_once(':')
            .ok_or(SignatureError::MissingSeparator)?;

        let mut buf = [0; 66];
        let mut bytes = [0; 64];
        match BASE64.decode_mut(bytes64.as_bytes(), &mut buf) {
            Ok(64) => {
                bytes.copy_from_slice(&buf[..64]);
            }
            Ok(n) => return Err(SignatureError::InvalidSignatureLen(n)),
            // keeping DecodePartial gets annoying lifetime-wise
            Err(_) => return Err(SignatureError::DecodeError(input.to_string())),
        }

        Ok(Signature { name, bytes })
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn bytes(&self) -> &[u8; 64] {
        &self.bytes
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("Missing separator")]
    MissingSeparator,
    #[error("Invalid signature len: {0}")]
    InvalidSignatureLen(usize),
    #[error("Unable to base64-decode signature: {0}")]
    DecodeError(String),
}

impl Display for Signature<'_> {
    fn fmt(&self, w: &mut fmt::Formatter) -> fmt::Result {
        write!(w, "{}:{}", self.name, BASE64.encode(&self.bytes))
    }
}
