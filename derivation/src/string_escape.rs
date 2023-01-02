const STRING_ESCAPER: [(char, &str); 5] = [
    ('\\', "\\\\"),
    ('\n', "\\n"),
    ('\r', "\\r"),
    ('\t', "\\t"),
    ('\"', "\\\""),
];

pub fn escape_string(s: &str) -> String {
    let mut s_replaced = s.to_string();

    for escape_sequence in STRING_ESCAPER {
        s_replaced = s_replaced.replace(escape_sequence.0, escape_sequence.1);
    }

    format!("\"{}\"", s_replaced)
}
