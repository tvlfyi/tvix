use tracing::trace;

use crate::nixbase32;

/// Parses a `14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0k.nar`
/// string and returns the nixbase32-decoded digest, as well as the compression
/// suffix (which might be empty).
pub fn parse_nar_str(s: &str) -> Option<([u8; 32], &str)> {
    if !s.is_char_boundary(52) {
        trace!("invalid string, no char boundary at 52");
        return None;
    }

    let (hash_str, suffix) = s.split_at(52);

    // we know hash_str is 52 bytes, so it's ok to unwrap here.
    let hash_str_fixed: [u8; 52] = hash_str.as_bytes().try_into().unwrap();

    match suffix.strip_prefix(".nar") {
        Some(compression_suffix) => match nixbase32::decode_fixed(hash_str_fixed) {
            Err(e) => {
                trace!(err=%e, "invalid nixbase32 encoding");
                None
            }
            Ok(digest) => Some((digest, compression_suffix)),
        },
        None => {
            trace!("no .nar suffix");
            None
        }
    }
}

/// Parses a `3mzh8lvgbynm9daj7c82k2sfsfhrsfsy.narinfo` string and returns the
/// nixbase32-decoded digest.
pub fn parse_narinfo_str(s: &str) -> Option<[u8; 20]> {
    if !s.is_char_boundary(32) {
        trace!("invalid string, no char boundary at 32");
        return None;
    }

    match s.split_at(32) {
        (hash_str, ".narinfo") => {
            // we know this is 32 bytes, so it's ok to unwrap here.
            let hash_str_fixed: [u8; 32] = hash_str.as_bytes().try_into().unwrap();

            match nixbase32::decode_fixed(hash_str_fixed) {
                Err(e) => {
                    trace!(err=%e, "invalid nixbase32 encoding");
                    None
                }
                Ok(digest) => Some(digest),
            }
        }
        _ => {
            trace!("invalid string, no .narinfo suffix");
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::{parse_nar_str, parse_narinfo_str};
    use hex_literal::hex;

    #[test]
    fn parse_nar_str_success() {
        assert_eq!(
            (
                hex!("13a8cf7ca57f68a9f1752acee36a72a55187d3a954443c112818926f26109d91"),
                ""
            ),
            parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0k.nar").unwrap()
        );

        assert_eq!(
            (
                hex!("13a8cf7ca57f68a9f1752acee36a72a55187d3a954443c112818926f26109d91"),
                ".xz"
            ),
            parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0k.nar.xz").unwrap()
        )
    }

    #[test]
    fn parse_nar_str_failure() {
        assert!(parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0").is_none());
        assert!(
            parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0🦊.nar").is_none()
        )
    }
    #[test]
    fn parse_narinfo_str_success() {
        assert_eq!(
            hex!("8a12321522fd91efbd60ebb2481af88580f61600"),
            parse_narinfo_str("00bgd045z0d4icpbc2yyz4gx48ak44la.narinfo").unwrap()
        );
    }

    #[test]
    fn parse_narinfo_str_failure() {
        assert!(parse_narinfo_str("00bgd045z0d4icpbc2yyz4gx48ak44la").is_none());
        assert!(parse_narinfo_str("/00bgd045z0d4icpbc2yyz4gx48ak44la").is_none());
        assert!(parse_narinfo_str("000000").is_none());
        assert!(parse_narinfo_str("00bgd045z0d4icpbc2yyz4gx48ak44l🦊.narinfo").is_none());
    }
}
