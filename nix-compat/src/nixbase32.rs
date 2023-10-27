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
    NonzeroCarry,
    #[error("invalid length")]
    InvalidLength,
}

/// Returns encoded input
pub fn encode(input: &[u8]) -> String {
    let output_len = encode_len(input.len());
    let mut output = String::with_capacity(output_len);

    for n in (0..output_len).rev() {
        let b = n * 5; // bit offset within the entire input
        let i = b / 8; // input byte index
        let j = b % 8; // bit offset within that input byte

        // 5-bit words aren't aligned to bytes
        // we can only read byte-aligned units
        // read 16 bits then shift and mask to 5
        let c = {
            let mut word = input[i] as u16;
            if let Some(&msb) = input.get(i + 1) {
                word |= (msb as u16) << 8;
            }
            (word >> j) & 0x1f
        };

        output.write_char(ALPHABET[c as usize] as char).unwrap();
    }

    output
}

/// This maps a nixbase32-encoded character to its binary representation, which
/// is also the index of the character in the alphabet. Invalid characters are
/// mapped to 0xFF, which is itself an invalid value.
const BASE32_ORD: [u8; 256] = {
    let mut ord = [0xFF; 256];
    let mut alphabet = ALPHABET.as_slice();
    let mut i = 0;

    while let &[c, ref tail @ ..] = alphabet {
        ord[c as usize] = i;
        alphabet = tail;
        i += 1;
    }

    ord
};

/// Returns decoded input
pub fn decode(input: impl AsRef<[u8]>) -> Result<Vec<u8>, Nixbase32DecodeError> {
    let input = input.as_ref();

    let output_len = decode_len(input.len());
    let mut output: Vec<u8> = vec![0x00; output_len];

    decode_inner(input, &mut output)?;
    Ok(output)
}

pub fn decode_fixed<const K: usize>(
    input: impl AsRef<[u8]>,
) -> Result<[u8; K], Nixbase32DecodeError> {
    let input = input.as_ref();

    if input.len() != encode_len(K) {
        return Err(Nixbase32DecodeError::InvalidLength);
    }

    let mut output = [0; K];
    decode_inner(input, &mut output)?;
    Ok(output)
}

fn decode_inner(input: &[u8], output: &mut [u8]) -> Result<(), Nixbase32DecodeError> {
    // loop over all characters in reverse, and keep the iteration count in n.
    let mut carry = 0;
    let mut mask = 0;
    for (n, &c) in input.iter().rev().enumerate() {
        let b = n * 5;
        let i = b / 8;
        let j = b % 8;

        let digit = BASE32_ORD[c as usize];
        let value = (digit as u16) << j;
        output[i] |= value as u8 | carry;
        carry = (value >> 8) as u8;

        mask |= digit;
    }

    if mask == 0xFF {
        let c = find_invalid(input);
        return Err(Nixbase32DecodeError::CharacterNotInAlphabet(c));
    }

    // if we're at the end, but have a nonzero carry, the encoding is invalid.
    if carry != 0 {
        return Err(Nixbase32DecodeError::NonzeroCarry);
    }

    Ok(())
}

fn find_invalid(input: &[u8]) -> u8 {
    for &c in input {
        if !ALPHABET.contains(&c) {
            return c;
        }
    }

    unreachable!()
}

/// Returns the decoded length of an input of length len.
pub fn decode_len(len: usize) -> usize {
    (len * 5) / 8
}

/// Returns the encoded length of an input of length len
pub fn encode_len(len: usize) -> usize {
    (len * 8 + 4) / 5
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;
    use test_case::test_case;

    #[test_case("", &[]; "empty bytes")]
    #[test_case("0z", &hex!("1f"); "one byte")]
    #[test_case("00bgd045z0d4icpbc2yyz4gx48ak44la", &hex!("8a12321522fd91efbd60ebb2481af88580f61600"); "store path")]
    #[test_case("0c5b8vw40dy178xlpddw65q9gf1h2186jcc3p4swinwggbllv8mk", &hex!("b3a24de97a8fdbc835b9833169501030b8977031bcb54b3b3ac13740f846ab30"); "sha256")]
    fn encode(enc: &str, dec: &[u8]) {
        assert_eq!(enc, super::encode(&dec));
    }

    #[test_case("", Some(&[]) ; "empty bytes")]
    #[test_case("0z", Some(&hex!("1f")); "one byte")]
    #[test_case("00bgd045z0d4icpbc2yyz4gx48ak44la", Some(&hex!("8a12321522fd91efbd60ebb2481af88580f61600")); "store path")]
    #[test_case("0c5b8vw40dy178xlpddw65q9gf1h2186jcc3p4swinwggbllv8mk", Some(&hex!("b3a24de97a8fdbc835b9833169501030b8977031bcb54b3b3ac13740f846ab30")); "sha256")]
    // this is invalid encoding, because it encodes 10 1-bits, so the carry
    // would be 2 1-bits
    #[test_case("zz", None; "invalid encoding-1")]
    // this is an even more specific example - it'd decode as 00000000 11
    #[test_case("c0", None; "invalid encoding-2")]

    fn decode(enc: &str, dec: Option<&[u8]>) {
        match dec {
            Some(dec) => {
                // The decode needs to match what's passed in dec
                assert_eq!(dec, super::decode(enc).unwrap());
            }
            None => {
                // the decode needs to be an error
                assert!(super::decode(enc).is_err());
            }
        }
    }

    #[test]
    fn decode_fixed() {
        assert_eq!(
            super::decode_fixed("00bgd045z0d4icpbc2yyz4gx48ak44la").unwrap(),
            hex!("8a12321522fd91efbd60ebb2481af88580f61600")
        );
        assert_eq!(
            super::decode_fixed::<32>("00").unwrap_err(),
            super::Nixbase32DecodeError::InvalidLength
        );
    }

    #[test]
    fn encode_len() {
        assert_eq!(super::encode_len(0), 0);
        assert_eq!(super::encode_len(20), 32);
    }

    #[test]
    fn decode_len() {
        assert_eq!(super::decode_len(0), 0);
        assert_eq!(super::decode_len(32), 20);
    }
}
