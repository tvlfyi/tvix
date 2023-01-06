//! Implements the slightly odd "base32" encoding that's used in Nix.
//!
//! Nix uses a custom alphabet. Contrary to other implementations (RFC4648),
//! encoding to "nix base32" doesn't use any padding, and reads in characters
//! in reverse order.
//!
//! This is also the main reason why `data_encoding::Encoding` can't be used
//! directly, but this module aims to provide a similar interface (with some
//! methods omitted).
use data_encoding::{DecodeError, Encoding, Specification};
use lazy_static::lazy_static;

/// Nixbase32Encoding wraps a data_encoding::Encoding internally.
/// We can't use it directly, as nix also reads in characters in reverse order.
pub struct Nixbase32Encoding {
    encoding: Encoding,
}

lazy_static! {
    /// Returns a Nixbase32Encoding providing some functions seen on a data_encoding::Encoding.
    pub static ref NIXBASE32: Nixbase32Encoding = nixbase32_encoding();
}

/// Populates the Nixbase32Encoding struct with a data_encoding::Encoding,
/// using the nixbase32 alphabet and config.
fn nixbase32_encoding() -> Nixbase32Encoding {
    let mut spec = Specification::new();
    spec.symbols.push_str("0123456789abcdfghijklmnpqrsvwxyz");

    Nixbase32Encoding {
        encoding: spec.encoding().unwrap(),
    }
}

impl Nixbase32Encoding {
    /// Returns encoded input
    pub fn encode(&self, input: &[u8]) -> String {
        // Reverse the input, reading in the bytes in reverse order.
        let reversed: Vec<u8> = input.iter().cloned().rev().collect();
        self.encoding.encode(&reversed)
    }

    /// Returns decoded input
    /// Check [data_encoding::Encoding::encode] for the error cases.
    pub fn decode(&self, input: &[u8]) -> Result<Vec<u8>, DecodeError> {
        // Decode first, then reverse the bytes of the output.
        let mut output = self.encoding.decode(input)?;
        output.reverse();
        Ok(output)
    }

    /// Returns the decoded length of an input of length len.
    /// Check [data_encoding::Encoding::decode_len] for the error cases.
    pub fn decode_len(&self, len: usize) -> Result<usize, DecodeError> {
        self.encoding.decode_len(len)
    }

    /// Returns the encoded length of an input of length len
    pub fn encode_len(&self, len: usize) -> usize {
        self.encoding.encode_len(len)
    }
}

#[cfg(test)]
mod tests {
    use crate::nixbase32::NIXBASE32;
    use test_case::test_case;

    #[test_case("", vec![] ; "empty bytes")]
    // FUTUREWORK: b/235
    // this seems to encode to 3w?
    // #[test_case("0z", vec![0x1f]; "one byte")]
    #[test_case("00bgd045z0d4icpbc2yyz4gx48ak44la", vec![
                 0x8a, 0x12, 0x32, 0x15, 0x22, 0xfd, 0x91, 0xef, 0xbd, 0x60, 0xeb, 0xb2, 0x48, 0x1a,
                 0xf8, 0x85, 0x80, 0xf6, 0x16, 0x00]; "store path")]
    fn encode(enc: &str, dec: Vec<u8>) {
        assert_eq!(enc, NIXBASE32.encode(&dec));
    }

    #[test_case("", Some(vec![]) ; "empty bytes")]
    // FUTUREWORK: b/235
    // this seems to require spec.check_trailing_bits and still fails?
    // #[test_case("0z", Some(vec![0x1f]); "one byte")]
    #[test_case("00bgd045z0d4icpbc2yyz4gx48ak44la", Some(vec![
                 0x8a, 0x12, 0x32, 0x15, 0x22, 0xfd, 0x91, 0xef, 0xbd, 0x60, 0xeb, 0xb2, 0x48, 0x1a,
                 0xf8, 0x85, 0x80, 0xf6, 0x16, 0x00]); "store path")]
    // this is invalid encoding, because it encodes 10 1-bytes, so the carry
    // would be 2 1-bytes
    #[test_case("zz", None; "invalid encoding-1")]
    // this is an even more specific example - it'd decode as 00000000 11
    // FUTUREWORK: b/235
    // #[test_case("c0", None; "invalid encoding-2")]

    fn decode(enc: &str, dec: Option<Vec<u8>>) {
        match dec {
            Some(dec) => {
                // The decode needs to match what's passed in dec
                assert_eq!(dec, NIXBASE32.decode(enc.as_bytes()).unwrap());
            }
            None => {
                // the decode needs to be an error
                assert_eq!(true, NIXBASE32.decode(enc.as_bytes()).is_err());
            }
        }
    }

    #[test]
    fn encode_len() {
        assert_eq!(NIXBASE32.encode_len(20), 32)
    }

    #[test]
    fn decode_len() {
        assert_eq!(NIXBASE32.decode_len(32).unwrap(), 20)
    }
}
