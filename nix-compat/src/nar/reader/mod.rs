//! Parser for the Nix archive format, aka NAR.
//!
//! NAR files (and their hashed representations) are used in C++ Nix for
//! a variety of things, including addressing fixed-output derivations
//! and transferring store paths between Nix stores.

use std::io::{
    self,
    ErrorKind::{InvalidData, UnexpectedEof},
    Read,
};

// Required reading for understanding this module.
use crate::nar::wire;

mod read;
#[cfg(test)]
mod test;

pub type Reader<'a> = dyn Read + Send + 'a;

/// Start reading a NAR file from `reader`.
pub fn open<'a, 'r>(reader: &'a mut Reader<'r>) -> io::Result<Node<'a, 'r>> {
    read::token(reader, &wire::TOK_NAR)?;
    Node::new(reader)
}

pub enum Node<'a, 'r> {
    Symlink {
        target: Vec<u8>,
    },
    File {
        executable: bool,
        reader: FileReader<'a, 'r>,
    },
    Directory(DirReader<'a, 'r>),
}

impl<'a, 'r> Node<'a, 'r> {
    /// Start reading a [Node], matching the next [wire::Node].
    ///
    /// Reading the terminating [wire::TOK_PAR] is done immediately for [Node::Symlink],
    /// but is otherwise left to [DirReader] or [FileReader].
    fn new(reader: &'a mut Reader<'r>) -> io::Result<Self> {
        Ok(match read::tag(reader)? {
            wire::Node::Sym => {
                let target = read::bytes(reader, wire::MAX_TARGET_LEN)?;

                if target.is_empty() || target.contains(&0) {
                    return Err(InvalidData.into());
                }

                read::token(reader, &wire::TOK_PAR)?;

                Node::Symlink { target }
            }
            tag @ (wire::Node::Reg | wire::Node::Exe) => {
                let len = read::u64(reader)?;

                Node::File {
                    executable: tag == wire::Node::Exe,
                    reader: FileReader::new(reader, len)?,
                }
            }
            wire::Node::Dir => Node::Directory(DirReader::new(reader)),
        })
    }
}

/// File contents, readable through the [Read] trait.
///
/// It comes with some caveats:
///  * You must always read the entire file, unless you intend to abandon the entire archive reader.
///  * You must abandon the entire archive reader upon the first error.
///
/// It's fine to read exactly `reader.len()` bytes without ever seeing an explicit EOF.
///
/// TODO(edef): enforce these in `#[cfg(debug_assertions)]`
pub struct FileReader<'a, 'r> {
    reader: &'a mut Reader<'r>,
    len: u64,
    /// Truncated original file length for padding computation.
    /// We only care about the 3 least significant bits; semantically, this is a u3.
    pad: u8,
}

impl<'a, 'r> FileReader<'a, 'r> {
    /// Instantiate a new reader, starting after [wire::TOK_REG] or [wire::TOK_EXE].
    /// We handle the terminating [wire::TOK_PAR] on semantic EOF.
    fn new(reader: &'a mut Reader<'r>, len: u64) -> io::Result<Self> {
        // For zero-length files, we have to read the terminating TOK_PAR
        // immediately, since FileReader::read may never be called; we've
        // already reached semantic EOF by definition.
        if len == 0 {
            read::token(reader, &wire::TOK_PAR)?;
        }

        Ok(Self {
            reader,
            len,
            pad: len as u8,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn len(&self) -> u64 {
        self.len
    }
}

impl Read for FileReader<'_, '_> {
    fn read(&mut self, mut buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.is_empty() {
            return Ok(0);
        }

        if buf.len() as u64 > self.len {
            buf = &mut buf[..self.len as usize];
        }

        let n = self.reader.read(buf)?;
        self.len -= n as u64;

        if n == 0 {
            return Err(UnexpectedEof.into());
        }

        // If we've reached semantic EOF, consume and verify the padding and terminating TOK_PAR.
        // Files are padded to 64 bits (8 bytes), just like any other byte string in the wire format.
        if self.is_empty() {
            let pad = (self.pad & 7) as usize;

            if pad != 0 {
                let mut buf = [0; 8];
                self.reader.read_exact(&mut buf[pad..])?;

                if buf != [0; 8] {
                    return Err(InvalidData.into());
                }
            }

            read::token(self.reader, &wire::TOK_PAR)?;
        }

        Ok(n)
    }
}

/// A directory iterator, yielding a sequence of [Node]s.
/// It must be fully consumed before reading further from the [DirReader] that produced it, if any.
pub struct DirReader<'a, 'r> {
    reader: &'a mut Reader<'r>,
    /// Previous directory entry name.
    /// We have to hang onto this to enforce name monotonicity.
    prev_name: Option<Vec<u8>>,
}

pub struct Entry<'a, 'r> {
    pub name: Vec<u8>,
    pub node: Node<'a, 'r>,
}

impl<'a, 'r> DirReader<'a, 'r> {
    fn new(reader: &'a mut Reader<'r>) -> Self {
        Self {
            reader,
            prev_name: None,
        }
    }

    /// Read the next [Entry] from the directory.
    ///
    /// We explicitly don't implement [Iterator], since treating this as
    /// a regular Rust iterator will surely lead you astray.
    ///
    ///  * You must always consume the entire iterator, unless you abandon the entire archive reader.
    ///  * You must abandon the entire archive reader on the first error.
    ///  * You must abandon the directory reader upon the first [None].
    ///  * Even if you know the amount of elements up front, you must keep reading until you encounter [None].
    ///
    /// TODO(edef): enforce these in `#[cfg(debug_assertions)]`
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<Option<Entry>> {
        // COME FROM the previous iteration: if we've already read an entry,
        // read its terminating TOK_PAR here.
        if self.prev_name.is_some() {
            read::token(self.reader, &wire::TOK_PAR)?;
        }

        // Determine if there are more entries to follow
        if let wire::Entry::None = read::tag(self.reader)? {
            // We've reached the end of this directory.
            return Ok(None);
        }

        let name = read::bytes(self.reader, wire::MAX_NAME_LEN)?;

        if name.is_empty()
            || name.contains(&0)
            || name.contains(&b'/')
            || name == b"."
            || name == b".."
        {
            return Err(InvalidData.into());
        }

        // Enforce strict monotonicity of directory entry names.
        match &mut self.prev_name {
            None => {
                self.prev_name = Some(name.clone());
            }
            Some(prev_name) => {
                if *prev_name >= name {
                    return Err(InvalidData.into());
                }

                name[..].clone_into(prev_name);
            }
        }

        read::token(self.reader, &wire::TOK_NOD)?;

        Ok(Some(Entry {
            name,
            node: Node::new(&mut self.reader)?,
        }))
    }
}
