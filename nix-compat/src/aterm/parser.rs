//! This module implements parsing code for some basic building blocks
//! of the [ATerm][] format, which is used by C++ Nix to serialize Derivations.
//!
//! [ATerm]: http://program-transformation.org/Tools/ATermFormat.html
use bstr::BString;
use nom::branch::alt;
use nom::bytes::complete::{escaped_transform, is_not, tag};
use nom::character::complete::char as nomchar;
use nom::combinator::{map, value};
use nom::multi::separated_list0;
use nom::sequence::delimited;
use nom::IResult;

/// Parse a bstr and undo any escaping.
fn parse_escaped_bstr(i: &[u8]) -> IResult<&[u8], BString> {
    escaped_transform(
        is_not("\"\\"),
        '\\',
        alt((
            value("\\".as_bytes(), nomchar('\\')),
            value("\n".as_bytes(), nomchar('n')),
            value("\t".as_bytes(), nomchar('t')),
            value("\r".as_bytes(), nomchar('r')),
            value("\"".as_bytes(), nomchar('\"')),
        )),
    )(i)
    .map(|(i, v)| (i, BString::new(v)))
}

/// Parse a field in double quotes, undo any escaping, and return the unquoted
/// and decoded Vec<u8>.
pub(crate) fn parse_bstr_field(i: &[u8]) -> IResult<&[u8], BString> {
    // inside double quotes…
    delimited(
        nomchar('\"'),
        // There is
        alt((
            // …either is a bstr after unescaping
            parse_escaped_bstr,
            // …or an empty string.
            map(tag(b""), |_| BString::default()),
        )),
        nomchar('\"'),
    )(i)
}

/// Parse a field in double quotes, undo any escaping, and return the unquoted
/// and decoded string, if it's a valid string. Or fail parsing if the bytes are
/// no valid UTF-8.
pub(crate) fn parse_string_field(i: &[u8]) -> IResult<&[u8], String> {
    // inside double quotes…
    delimited(
        nomchar('\"'),
        // There is
        alt((
            // either is a String after unescaping
            nom::combinator::map_opt(parse_escaped_bstr, |escaped_bstr| {
                String::from_utf8(escaped_bstr.into()).ok()
            }),
            // or an empty string.
            map(tag(b""), |_| String::new()),
        )),
        nomchar('\"'),
    )(i)
}

/// Parse a list of of string fields (enclosed in brackets)
pub(crate) fn parse_str_list(i: &[u8]) -> IResult<&[u8], Vec<String>> {
    // inside brackets
    delimited(
        nomchar('['),
        separated_list0(nomchar(','), parse_string_field),
        nomchar(']'),
    )(i)
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    #[test_case(br#""""#, b"", b""; "empty")]
    #[test_case(br#""Hello World""#, b"Hello World", b""; "hello world")]
    #[test_case(br#""\"""#, br#"""#, b""; "doublequote")]
    #[test_case(br#"":""#, b":", b""; "colon")]
    #[test_case(br#""\""Rest"#, br#"""#, b"Rest"; "doublequote rest")]
    fn parse_bstr_field(input: &[u8], expected: &[u8], exp_rest: &[u8]) {
        let (rest, parsed) = super::parse_bstr_field(input).expect("must parse");
        assert_eq!(exp_rest, rest, "expected remainder");
        assert_eq!(expected, parsed);
    }

    #[test_case(br#""""#, "", b""; "empty")]
    #[test_case(br#""Hello World""#, "Hello World", b""; "hello world")]
    #[test_case(br#""\"""#, r#"""#, b""; "doublequote")]
    #[test_case(br#"":""#, ":", b""; "colon")]
    #[test_case(br#""\""Rest"#, r#"""#, b"Rest"; "doublequote rest")]
    fn parse_string_field(input: &[u8], expected: &str, exp_rest: &[u8]) {
        let (rest, parsed) = super::parse_string_field(input).expect("must parse");
        assert_eq!(exp_rest, rest, "expected remainder");
        assert_eq!(expected, &parsed);
    }

    #[test]
    fn parse_string_field_invalid_encoding_fail() {
        let input: Vec<_> = vec![b'"', 0xc5, 0xc4, 0xd6, b'"'];

        super::parse_string_field(&input).expect_err("must fail");
    }

    #[test_case(br#"["foo"]"#, vec!["foo".to_string()], b""; "single foo")]
    #[test_case(b"[]", vec![], b""; "empty list")]
    #[test_case(b"[]blub", vec![], b"blub"; "empty list with rest")]
    fn parse_list(input: &[u8], expected: Vec<String>, exp_rest: &[u8]) {
        let (rest, parsed) = super::parse_str_list(input).expect("must parse");
        assert_eq!(exp_rest, rest, "expected remainder");
        assert_eq!(expected, parsed);
    }
}
