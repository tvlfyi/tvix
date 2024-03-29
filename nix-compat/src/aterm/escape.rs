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
    use test_case::test_case;

    #[test_case(b"", b""; "empty")]
    #[test_case(b"\"", b"\\\""; "doublequote")]
    #[test_case(b":", b":"; "colon")]
    fn escape(input: &[u8], expected: &[u8]) {
        assert_eq!(expected, escape_bytes(input))
    }
}
