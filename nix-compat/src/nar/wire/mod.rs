//! NAR wire format, without I/O details, since those differ between
//! the synchronous and asynchronous implementations.
//!
//! The wire format is an S-expression format, encoded onto the wire
//! using simple encoding rules.
//!
//! # Encoding
//!
//! Lengths are represented as 64-bit unsigned integers in little-endian
//! format. Byte strings, including file contents and syntactic strings
//! part of the grammar, are prefixed by their 64-bit length, and padded
//! to 8-byte (64-bit) alignment with zero bytes. The zero-length string
//! is therefore encoded as eight zero bytes representing its length.
//!
//! # Grammar
//!
//! The NAR grammar is as follows:
//! ```plain
//! archive ::= "nix-archive-1" node
//!
//! node ::= "(" "type" "symlink" "target" string ")"
//!      ||= "(" "type" "regular" ("executable" "")? "contents" string ")"
//!      ||= "(" "type" "directory" entry* ")"
//!
//! entry ::= "entry" "(" "name" string "node" node ")"
//! ```
//!
//! We rewrite it to pull together the purely syntactic elements into
//! unified tokens, producing an equivalent grammar that can be parsed
//! and serialized more elegantly:
//! ```plain
//! archive ::= TOK_NAR node
//! node ::= TOK_SYM string             TOK_PAR
//!      ||= (TOK_REG | TOK_EXE) string TOK_PAR
//!      ||= TOK_DIR entry*             TOK_PAR
//!
//! entry ::= TOK_ENT string TOK_NOD node TOK_PAR
//!
//! TOK_NAR ::= "nix-archive-1" "(" "type"
//! TOK_SYM ::= "symlink" "target"
//! TOK_REG ::= "regular" "contents"
//! TOK_EXE ::= "regular" "executable" ""
//! TOK_DIR ::= "directory"
//! TOK_ENT ::= "entry" "(" "name"
//! TOK_NOD ::= "node" "(" "type"
//! TOK_PAR ::= ")"
//! ```
//!
//! # Restrictions
//!
//! NOTE: These restrictions are not (and cannot be) enforced by this module,
//! but must be enforced by its consumers, [super::reader] and [super::writer].
//!
//! Directory entry names cannot have the reserved names `.` and `..`, nor contain
//! forward slashes. They must appear in strictly ascending lexicographic order
//! within a directory, and can be at most [MAX_NAME_LEN] bytes in length.
//!
//! Symlink targets can be at most [MAX_TARGET_LEN] bytes in length.
//!
//! Neither is permitted to be empty, or contain null bytes.

// These values are the standard Linux length limits
/// Maximum length of a directory entry name
pub const MAX_NAME_LEN: usize = 255;
/// Maximum length of a symlink target
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
pub const TOK_SYM: [u8; 32] = *b"\x07\0\0\0\0\0\0\0symlink\0\x06\0\0\0\0\0\0\0target\0\0";
pub const TOK_REG: [u8; 32] = *b"\x07\0\0\0\0\0\0\0regular\0\x08\0\0\0\0\0\0\0contents";
pub const TOK_EXE: [u8; 64] = *b"\x07\0\0\0\0\0\0\0regular\0\x0a\0\0\0\0\0\0\0executable\0\0\0\0\0\0\0\0\0\0\0\0\0\0\x08\0\0\0\0\0\0\0contents";
pub const TOK_DIR: [u8; 24] = *b"\x09\0\0\0\0\0\0\0directory\0\0\0\0\0\0\0";
pub const TOK_ENT: [u8; 48] = *b"\x05\0\0\0\0\0\0\0entry\0\0\0\x01\0\0\0\0\0\0\0(\0\0\0\0\0\0\0\x04\0\0\0\0\0\0\0name\0\0\0\0";
pub const TOK_NOD: [u8; 48] = *b"\x04\0\0\0\0\0\0\0node\0\0\0\0\x01\0\0\0\0\0\0\0(\0\0\0\0\0\0\0\x04\0\0\0\0\0\0\0type\0\0\0\0";
pub const TOK_PAR: [u8; 16] = *b"\x01\0\0\0\0\0\0\0)\0\0\0\0\0\0\0";

#[test]
fn tokens() {
    let cases: &[(&[u8], &[&str])] = &[
        (&TOK_NAR, &["nix-archive-1", "(", "type"]),
        (&TOK_SYM, &["symlink", "target"]),
        (&TOK_REG, &["regular", "contents"]),
        (&TOK_EXE, &["regular", "executable", "", "contents"]),
        (&TOK_DIR, &["directory"]),
        (&TOK_ENT, &["entry", "(", "name"]),
        (&TOK_NOD, &["node", "(", "type"]),
        (&TOK_PAR, &[")"]),
    ];

    for &(tok, xs) in cases {
        assert_eq!(tok, token(xs));
    }
}

pub use tag::Tag;
mod tag;

tag::make! {
    /// These are the node tokens, succeeding [TOK_NAR] or [TOK_NOD],
    /// and preceding the next variable-length element.
    pub enum Node[16] {
        Sym = TOK_SYM,
        Reg = TOK_REG,
        Exe = TOK_EXE,
        Dir = TOK_DIR,
    }

    /// Directory entry or terminator
    pub enum Entry[0] {
        /// End of directory
        None = TOK_PAR,
        /// Directory entry
        /// Followed by a name string, [TOK_NOD], and a [Node].
        Some = TOK_ENT,
    }
}
