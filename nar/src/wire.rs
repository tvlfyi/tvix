pub const MAX_NAME_LEN: usize = 255;
pub const MAX_TARGET_LEN: usize = 4095;

#[cfg(test)]
fn token(xs: &[&str]) -> Vec<u8> {
    let mut out = vec![];
    for x in xs {
        let len = x.len() as u64;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(x.as_bytes());

        let n = x.len() & 7;
        if n != 0 {
            const ZERO: [u8; 8] = [0; 8];
            out.extend_from_slice(&ZERO[n..]);
        }
    }
    out
}

pub const TOK_NAR: [u8; 56] = *b"\x0d\0\0\0\0\0\0\0nix-archive-1\0\0\0\x01\0\0\0\0\0\0\0(\0\0\0\0\0\0\0\x04\0\0\0\0\0\0\0type\0\0\0\0";
pub const TOK_REG: [u8; 32] = *b"\x07\0\0\0\0\0\0\0regular\0\x08\0\0\0\0\0\0\0contents";
pub const TOK_EXE: [u8; 64] = *b"\x07\0\0\0\0\0\0\0regular\0\x0a\0\0\0\0\0\0\0executable\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x08\0\0\0\0\0\0\0contents";
pub const TOK_SYM: [u8; 32] = *b"\x07\0\0\0\0\0\0\0symlink\0\x06\0\0\0\0\0\0\0target\0\0";
pub const TOK_DIR: [u8; 24] = *b"\x09\0\0\0\0\0\0\0directory\0\0\0\0\0\0\0";
pub const TOK_ENT: [u8; 48] = *b"\x05\0\0\0\0\0\0\0entry\0\0\0\x01\0\0\0\0\0\0\0(\0\0\0\0\0\0\0\x04\0\0\0\0\0\0\0name\0\0\0\0";
pub const TOK_NOD: [u8; 48] = *b"\x04\0\0\0\0\0\0\0node\0\0\0\0\x01\0\0\0\0\0\0\0(\0\0\0\0\0\0\0\x04\0\0\0\0\0\0\0type\0\0\0\0";
pub const TOK_PAR: [u8; 16] = *b"\x01\0\0\0\0\0\0\0)\0\0\0\0\0\0\0";

#[test]
fn tokens() {
    let cases: &[(&[u8], &[&str])] = &[
        (&TOK_NAR, &["nix-archive-1", "(", "type"]),
        (&TOK_REG, &["regular", "contents"]),
        (&TOK_EXE, &["regular", "executable", "", "contents"]),
        (&TOK_SYM, &["symlink", "target"]),
        (&TOK_DIR, &["directory"]),
        (&TOK_ENT, &["entry", "(", "name"]),
        (&TOK_NOD, &["node", "(", "type"]),
        (&TOK_PAR, &[")"]),
    ];

    for &(tok, xs) in cases {
        assert_eq!(tok, token(xs));
    }
}
