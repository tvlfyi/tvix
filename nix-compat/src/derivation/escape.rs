use bstr::{BString, ByteSlice};

pub fn escape_bstr(s: &[u8]) -> BString {
    let mut s: Vec<u8> = s.to_owned();

    s = s.replace(b"\\", b"\\\\");
    s = s.replace(b"\n", b"\\n");
    s = s.replace(b"\r", b"\\r");
    s = s.replace(b"\t", b"\\t");
    s = s.replace(b"\"", b"\\\"");

    let mut out: Vec<u8> = Vec::new();
    out.push(b'\"');
    out.append(&mut s);
    out.push(b'\"');

    out.into()
}

#[cfg(test)]
mod tests {
    use super::escape_bstr;
    use test_case::test_case;

    #[test_case(b"", b"\"\""; "empty")]
    #[test_case(b"\"", b"\"\\\"\""; "doublequote")]
    #[test_case(b":", b"\":\""; "colon")]
    fn escape(input: &[u8], expected: &[u8]) {
        assert_eq!(expected, escape_bstr(input))
    }
}
