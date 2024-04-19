use bstr::ByteSlice;

/// Escapes a byte sequence. Does not add surrounding quotes.
pub fn escape_bytes<P: AsRef<[u8]>>(s: P) -> Vec<u8> {
    let mut s: Vec<u8> = s.as_ref().to_vec();

    s = s.replace(b"\\", b"\\\\");
    s = s.replace(b"\n", b"\\n");
    s = s.replace(b"\r", b"\\r");
    s = s.replace(b"\t", b"\\t");
    s = s.replace(b"\"", b"\\\"");

    s
}

#[cfg(test)]
mod tests {
    use super::escape_bytes;
    use rstest::rstest;

    #[rstest]
    #[case::empty(b"", b"")]
    #[case::doublequote(b"\"", b"\\\"")]
    #[case::colon(b":", b":")]
    fn escape(#[case] input: &[u8], #[case] expected: &[u8]) {
        assert_eq!(expected, escape_bytes(input))
    }
}
