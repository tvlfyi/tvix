//! Implements an interface for writing the Nix archive format (NAR).
//!
//! NAR files (and their hashed representations) are used in C++ Nix for
//! addressing fixed-output derivations and a variety of other things.
//!
//! NAR files can be output to any type that implements [`Write`], and content
//! can be read from any type that implementes [`BufRead`].
//!
//! Writing a single file might look like this:
//!
//! ```rust
//! # use std::io::BufReader;
//! # let some_file: Vec<u8> = vec![0, 1, 2, 3, 4];
//!
//! // Output location to write the NAR to.
//! let mut sink: Vec<u8> = Vec::new();
//!
//! // Instantiate writer for this output location.
//! let mut nar = nix_compat::nar::writer::open(&mut sink)?;
//!
//! // Acquire metadata for the single file to output, and pass it in a
//! // `BufRead`-implementing type.
//!
//! let executable = false;
//! let size = some_file.len() as u64;
//! let mut reader = BufReader::new(some_file.as_slice());
//! nar.file(executable, size, &mut reader)?;
//! # Ok::<(), std::io::Error>(())
//! ```

use std::io::{
    self, BufRead,
    ErrorKind::{InvalidInput, UnexpectedEof},
    Write,
};

mod wire;

/// Convenience type alias for types implementing [`Write`].
pub type Writer<'a> = dyn Write + Send + 'a;

/// Create a new NAR, writing the output to the specified writer.
pub fn open<'a, 'w: 'a>(writer: &'a mut Writer<'w>) -> io::Result<Node<'a, 'w>> {
    let mut node = Node { writer };
    node.write(&wire::TOK_NAR)?;
    Ok(node)
}

/// Single node in a NAR file.
///
/// A NAR can be thought of as a tree of nodes represented by this type. Each
/// node can be a file, a symlink or a directory containing other nodes.
pub struct Node<'a, 'w: 'a> {
    writer: &'a mut Writer<'w>,
}

impl<'a, 'w> Node<'a, 'w> {
    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        self.writer.write_all(data)
    }

    fn pad(&mut self, n: u64) -> io::Result<()> {
        match (n & 7) as usize {
            0 => Ok(()),
            n => self.write(&[0; 8][n..]),
        }
    }

    /// Make this node a symlink.
    pub fn symlink(mut self, target: &[u8]) -> io::Result<()> {
        debug_assert!(
            target.len() <= wire::MAX_TARGET_LEN,
            "target.len() > {}",
            wire::MAX_TARGET_LEN
        );
        debug_assert!(
            !target.contains(&b'\0'),
            "invalid target characters: {target:?}"
        );
        debug_assert!(!target.is_empty(), "empty target");

        self.write(&wire::TOK_SYM)?;
        self.write(&target.len().to_le_bytes())?;
        self.write(target)?;
        self.pad(target.len() as u64)?;
        self.write(&wire::TOK_PAR)?;
        Ok(())
    }

    /// Make this node a single file.
    pub fn file(mut self, executable: bool, size: u64, reader: &mut dyn BufRead) -> io::Result<()> {
        self.write(if executable {
            &wire::TOK_EXE
        } else {
            &wire::TOK_REG
        })?;

        self.write(&size.to_le_bytes())?;

        let mut need = size;
        while need != 0 {
            let data = reader.fill_buf()?;

            if data.is_empty() {
                return Err(UnexpectedEof.into());
            }

            let n = need.min(data.len() as u64) as usize;
            self.write(&data[..n])?;

            need -= n as u64;
            reader.consume(n);
        }

        // bail if there's still data left in the passed reader.
        // This uses the same code as [BufRead::has_data_left] (unstable).
        if reader.fill_buf().map(|b| !b.is_empty())? {
            return Err(io::Error::new(
                InvalidInput,
                "reader contained more data than specified size",
            ));
        }

        self.pad(size)?;
        self.write(&wire::TOK_PAR)?;

        Ok(())
    }

    /// Make this node a directory, the content of which is set using the
    /// resulting [`Directory`] value.
    pub fn directory(mut self) -> io::Result<Directory<'a, 'w>> {
        self.write(&wire::TOK_DIR)?;
        Ok(Directory::new(self))
    }
}

#[cfg(debug_assertions)]
type Name = Vec<u8>;
#[cfg(not(debug_assertions))]
type Name = ();

fn into_name(_name: &[u8]) -> Name {
    #[cfg(debug_assertions)]
    _name.to_owned()
}

/// Content of a NAR node that represents a directory.
pub struct Directory<'a, 'w> {
    node: Node<'a, 'w>,
    prev_name: Option<Name>,
}

impl<'a, 'w> Directory<'a, 'w> {
    fn new(node: Node<'a, 'w>) -> Self {
        Self {
            node,
            prev_name: None,
        }
    }

    /// Add an entry to the directory.
    ///
    /// The entry is simply another [`Node`], which can then be filled like the
    /// root of a NAR (including, of course, by nesting directories).
    pub fn entry(&mut self, name: &[u8]) -> io::Result<Node<'_, 'w>> {
        debug_assert!(
            name.len() <= wire::MAX_NAME_LEN,
            "name.len() > {}",
            wire::MAX_NAME_LEN
        );
        debug_assert!(name != b"", "name may not be empty");
        debug_assert!(name != b".", "invalid name: {name:?}");
        debug_assert!(name != b"..", "invalid name: {name:?}");

        debug_assert!(!name.contains(&b'/'), "invalid name characters: {name:?}");
        debug_assert!(!name.contains(&b'\0'), "invalid name characters: {name:?}");

        match self.prev_name {
            None => {
                self.prev_name = Some(into_name(name));
            }
            Some(ref mut _prev_name) => {
                #[cfg(debug_assertions)]
                {
                    assert!(
                        &**_prev_name < name,
                        "misordered names: {_prev_name:?} >= {name:?}"
                    );
                    _prev_name.clear();
                    _prev_name.extend_from_slice(name);
                }
                self.node.write(&wire::TOK_PAR)?;
            }
        }

        self.node.write(&wire::TOK_ENT)?;
        self.node.write(&name.len().to_le_bytes())?;
        self.node.write(name)?;
        self.node.pad(name.len() as u64)?;
        self.node.write(&wire::TOK_NOD)?;

        Ok(Node {
            writer: &mut *self.node.writer,
        })
    }

    /// Close a directory and write terminators for the directory to the NAR.
    ///
    /// **Important:** This *must* be called when all entries have been written
    /// in a directory, otherwise the resulting NAR file will be invalid.
    pub fn close(mut self) -> io::Result<()> {
        if self.prev_name.is_some() {
            self.node.write(&wire::TOK_PAR)?;
        }

        self.node.write(&wire::TOK_PAR)?;
        Ok(())
    }
}
