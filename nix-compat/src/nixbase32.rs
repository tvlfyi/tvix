//! Implements the slightly odd "base32" encoding that's used in Nix.
//!
//! Nix uses a custom alphabet. Contrary to other implementations (RFC4648),
//! encoding to "nix base32" doesn't use any padding, and reads in characters
//! in reverse order.
//!
//! This is also the main reason why we can't use `data_encoding::Encoding` -
//! it gets things wrong if there normally would be a need for padding.

use std::fmt::Write;

use thiserror::Error;

const ALPHABET: &[u8; 32] = b"0123456789abcdfghijklmnpqrsvwxyz";

/// Errors that can occur while decoding nixbase32-encoded data.
#[derive(Debug, Eq, PartialEq, Error)]
pub enum Nixbase32DecodeError {
    #[error("character {0:x} not in alphabet")]
    CharacterNotInAlphabet(u8),
    #[error("nonzero carry")]
    NonzeroCarry(),
}

/// Returns encoded input
pub fn encode(input: &[u8]) -> String {
    let output_len = encode_len(input.len());
    let mut output = String::with_capacity(output_len);

    if output_len > 0 {
        for n in (0..=output_len - 1).rev() {
            let b = n * 5; // bit offset within the entire input
            let i = b / 8; // input byte index
            let j = b % 8; // bit offset within that input byte

            let mut c = input[i] >> j;
            if i + 1 < input.len() {
                // we want to right shift, and discard shifted out bits (unchecked)
                // To do this without panicing, we need to do the shifting in u16
                // and convert back to u8 afterwards.
                c |= ((input[i + 1] as u16) << (8 - j as u16)) as u8
            }

            output
                .write_char(ALPHABET[(c & 0x1f) as usize] as char)
                .unwrap();
        }
    }

    output
}

/// This maps a nixbase32-encoded character to its binary representation, which
/// is also the index of the character in the alphabet.
fn decode_char(encoded_char: &u8) -> Option<u8> {
    Some(match encoded_char {
        b'0'..=b'9' => encoded_char - b'0',
        b'a'..=b'd' => encoded_char - b'a' + 10_u8,
        b'f'..=b'n' => encoded_char - b'f' + 14_u8,
        b'p'..=b's' => encoded_char - b'p' + 23_u8,
        b'v'..=b'z' => encoded_char - b'v' + 27_u8,
        _ => return None,
    })
}

/// Returns decoded input
pub fn decode(input: &[u8]) -> Result<Vec<u8>, Nixbase32DecodeError> {
    let output_len = decode_len(input.len());
    let mut output: Vec<u8> = vec![0x00; output_len];

    // loop over all characters in reverse, and keep the iteration count in n.
    for (n, c) in input.iter().rev().enumerate() {
        match decode_char(c) {
            None => return Err(Nixbase32DecodeError::CharacterNotInAlphabet(*c)),
            Some(c_decoded) => {
                let b = n * 5;
                let i = b / 8;
                let j = b % 8;

                let val = (c_decoded as u16).rotate_left(j as u32);
                output[i] |= (val & 0x00ff) as u8;
                let carry = ((val & 0xff00) >> 8) as u8;

                // if we're at the end of dst…
                if i == output_len - 1 {
                    // but have a nonzero carry, the encoding is invalid.
                    if carry != 0 {
                        return Err(Nixbase32DecodeError::NonzeroCarry());
                    }
                } else {
                    output[i + 1] |= carry;
                }
            }
        }
    }

    Ok(output)
}

/// Returns the decoded length of an input of length len.
pub fn decode_len(len: usize) -> usize {
    (len * 5) / 8
}

/// Returns the encoded length of an input of length len
pub fn encode_len(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (len * 8 - 1) / 5 + 1
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    #[test_case("", vec![] ; "empty bytes")]
    #[test_case("0z", vec![0x1f]; "one byte")]
    #[test_case("00bgd045z0d4icpbc2yyz4gx48ak44la", vec![
                 0x8a, 0x12, 0x32, 0x15, 0x22, 0xfd, 0x91, 0xef, 0xbd, 0x60, 0xeb, 0xb2, 0x48, 0x1a,
                 0xf8, 0x85, 0x80, 0xf6, 0x16, 0x00]; "store path")]
    #[test_case("0c5b8vw40dy178xlpddw65q9gf1h2186jcc3p4swinwggbllv8mk", vec![
        0xb3, 0xa2, 0x4d, 0xe9, 0x7a, 0x8f, 0xdb, 0xc8, 0x35, 0xb9, 0x83, 0x31, 0x69, 0x50, 0x10, 0x30,
        0xb8, 0x97, 0x70, 0x31, 0xbc, 0xb5, 0x4b, 0x3b, 0x3a, 0xc1, 0x37, 0x40, 0xf8, 0x46, 0xab, 0x30,
    ]; "sha256")]
    fn encode(enc: &str, dec: Vec<u8>) {
        assert_eq!(enc, super::encode(&dec));
    }

    #[test_case("", Some(vec![]) ; "empty bytes")]
    #[test_case("0z", Some(vec![0x1f]); "one byte")]
    #[test_case("00bgd045z0d4icpbc2yyz4gx48ak44la", Some(vec![
                 0x8a, 0x12, 0x32, 0x15, 0x22, 0xfd, 0x91, 0xef, 0xbd, 0x60, 0xeb, 0xb2, 0x48, 0x1a,
                 0xf8, 0x85, 0x80, 0xf6, 0x16, 0x00]); "store path")]
    #[test_case("0c5b8vw40dy178xlpddw65q9gf1h2186jcc3p4swinwggbllv8mk", Some(vec![
        0xb3, 0xa2, 0x4d, 0xe9, 0x7a, 0x8f, 0xdb, 0xc8, 0x35, 0xb9, 0x83, 0x31, 0x69, 0x50, 0x10, 0x30,
        0xb8, 0x97, 0x70, 0x31, 0xbc, 0xb5, 0x4b, 0x3b, 0x3a, 0xc1, 0x37, 0x40, 0xf8, 0x46, 0xab, 0x30,
    ]); "sha256")]
    // this is invalid encoding, because it encodes 10 1-bits, so the carry
    // would be 2 1-bits
    #[test_case("zz", None; "invalid encoding-1")]
    // this is an even more specific example - it'd decode as 00000000 11
    #[test_case("c0", None; "invalid encoding-2")]

    fn decode(enc: &str, dec: Option<Vec<u8>>) {
        match dec {
            Some(dec) => {
                // The decode needs to match what's passed in dec
                assert_eq!(dec, super::decode(enc.as_bytes()).unwrap());
            }
            None => {
                // the decode needs to be an error
                assert!(super::decode(enc.as_bytes()).is_err());
            }
        }
    }

    #[test]
    fn encode_len() {
        assert_eq!(super::encode_len(20), 32)
    }

    #[test]
    fn decode_len() {
        assert_eq!(super::decode_len(32), 20)
    }
}
