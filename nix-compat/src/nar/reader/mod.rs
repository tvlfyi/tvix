//! Parser for the Nix archive format, aka NAR.
//!
//! NAR files (and their hashed representations) are used in C++ Nix for
//! a variety of things, including addressing fixed-output derivations
//! and transferring store paths between Nix stores.

use std::io::{
    self, BufRead,
    ErrorKind::{InvalidData, UnexpectedEof},
    Read, Write,
};

#[cfg(not(debug_assertions))]
use std::marker::PhantomData;

// Required reading for understanding this module.
use crate::nar::wire;

#[cfg(all(feature = "async", feature = "wire"))]
pub mod r#async;

mod read;
#[cfg(test)]
mod test;

pub type Reader<'a> = dyn BufRead + Send + 'a;

struct ArchiveReader<'a, 'r> {
    inner: &'a mut Reader<'r>,

    /// In debug mode, also track when we need to abandon this archive reader.
    /// The archive reader must be abandoned when:
    ///   * An error is encountered at any point
    ///   * A file or directory reader is dropped before being read entirely.
    /// All of these checks vanish in release mode.
    status: ArchiveReaderStatus<'a>,
}

macro_rules! try_or_poison {
    ($it:expr, $ex:expr) => {
        match $ex {
            Ok(x) => x,
            Err(e) => {
                $it.status.poison();
                return Err(e.into());
            }
        }
    };
}
/// Start reading a NAR file from `reader`.
pub fn open<'a, 'r>(reader: &'a mut Reader<'r>) -> io::Result<Node<'a, 'r>> {
    read::token(reader, &wire::TOK_NAR)?;
    Node::new(ArchiveReader {
        inner: reader,
        status: ArchiveReaderStatus::top(),
    })
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
    fn new(mut reader: ArchiveReader<'a, 'r>) -> io::Result<Self> {
        Ok(match read::tag(reader.inner)? {
            wire::Node::Sym => {
                let target =
                    try_or_poison!(reader, read::bytes(reader.inner, wire::MAX_TARGET_LEN));

                if target.is_empty() || target.contains(&0) {
                    reader.status.poison();
                    return Err(InvalidData.into());
                }

                try_or_poison!(reader, read::token(reader.inner, &wire::TOK_PAR));
                reader.status.ready_parent(); // Immediately allow reading from parent again

                Node::Symlink { target }
            }
            tag @ (wire::Node::Reg | wire::Node::Exe) => {
                let len = try_or_poison!(&mut reader, read::u64(reader.inner));

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
pub struct FileReader<'a, 'r> {
    reader: ArchiveReader<'a, 'r>,
    len: u64,
    /// Truncated original file length for padding computation.
    /// We only care about the 3 least significant bits; semantically, this is a u3.
    pad: u8,
}

impl<'a, 'r> FileReader<'a, 'r> {
    /// Instantiate a new reader, starting after [wire::TOK_REG] or [wire::TOK_EXE].
    /// We handle the terminating [wire::TOK_PAR] on semantic EOF.
    fn new(mut reader: ArchiveReader<'a, 'r>, len: u64) -> io::Result<Self> {
        // For zero-length files, we have to read the terminating TOK_PAR
        // immediately, since FileReader::read may never be called; we've
        // already reached semantic EOF by definition.
        if len == 0 {
            read::token(reader.inner, &wire::TOK_PAR)?;
            reader.status.ready_parent();
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

impl FileReader<'_, '_> {
    /// Equivalent to [BufRead::fill_buf]
    ///
    /// We can't directly implement [BufRead], because [FileReader::consume] needs
    /// to perform fallible I/O.
    pub fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.is_empty() {
            return Ok(&[]);
        }

        self.reader.check_correct();

        let mut buf = try_or_poison!(self.reader, self.reader.inner.fill_buf());

        if buf.is_empty() {
            self.reader.status.poison();
            return Err(UnexpectedEof.into());
        }

        if buf.len() as u64 > self.len {
            buf = &buf[..self.len as usize];
        }

        Ok(buf)
    }

    /// Analogous to [BufRead::consume], differing only in that it needs
    /// to perform I/O in order to read padding and terminators.
    pub fn consume(&mut self, n: usize) -> io::Result<()> {
        if n == 0 {
            return Ok(());
        }

        self.reader.check_correct();

        self.len = self
            .len
            .checked_sub(n as u64)
            .expect("consumed bytes past EOF");

        self.reader.inner.consume(n);

        if self.is_empty() {
            self.finish()?;
        }

        Ok(())
    }

    /// Copy the (remaining) contents of the file into `dst`.
    pub fn copy(&mut self, mut dst: impl Write) -> io::Result<()> {
        while !self.is_empty() {
            let buf = self.fill_buf()?;
            let n = try_or_poison!(self.reader, dst.write(buf));
            self.consume(n)?;
        }

        Ok(())
    }
}

impl Read for FileReader<'_, '_> {
    fn read(&mut self, mut buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.is_empty() {
            return Ok(0);
        }

        self.reader.check_correct();

        if buf.len() as u64 > self.len {
            buf = &mut buf[..self.len as usize];
        }

        let n = try_or_poison!(self.reader, self.reader.inner.read(buf));
        self.len -= n as u64;

        if n == 0 {
            self.reader.status.poison();
            return Err(UnexpectedEof.into());
        }

        if self.is_empty() {
            self.finish()?;
        }

        Ok(n)
    }
}

impl FileReader<'_, '_> {
    /// We've reached semantic EOF, consume and verify the padding and terminating TOK_PAR.
    /// Files are padded to 64 bits (8 bytes), just like any other byte string in the wire format.
    fn finish(&mut self) -> io::Result<()> {
        let pad = (self.pad & 7) as usize;

        if pad != 0 {
            let mut buf = [0; 8];
            try_or_poison!(self.reader, self.reader.inner.read_exact(&mut buf[pad..]));

            if buf != [0; 8] {
                self.reader.status.poison();
                return Err(InvalidData.into());
            }
        }

        try_or_poison!(self.reader, read::token(self.reader.inner, &wire::TOK_PAR));

        // Done with reading this file, allow going back up the chain of readers
        self.reader.status.ready_parent();

        Ok(())
    }
}

/// A directory iterator, yielding a sequence of [Node]s.
/// It must be fully consumed before reading further from the [DirReader] that produced it, if any.
pub struct DirReader<'a, 'r> {
    reader: ArchiveReader<'a, 'r>,
    /// Previous directory entry name.
    /// We have to hang onto this to enforce name monotonicity.
    prev_name: Vec<u8>,
}

pub struct Entry<'a, 'r> {
    pub name: &'a [u8],
    pub node: Node<'a, 'r>,
}

impl<'a, 'r> DirReader<'a, 'r> {
    fn new(reader: ArchiveReader<'a, 'r>) -> Self {
        Self {
            reader,
            prev_name: vec![],
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
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<Option<Entry<'_, 'r>>> {
        self.reader.check_correct();

        // COME FROM the previous iteration: if we've already read an entry,
        // read its terminating TOK_PAR here.
        if !self.prev_name.is_empty() {
            try_or_poison!(self.reader, read::token(self.reader.inner, &wire::TOK_PAR));
        }

        // Determine if there are more entries to follow
        if let wire::Entry::None = try_or_poison!(self.reader, read::tag(self.reader.inner)) {
            // We've reached the end of this directory.
            self.reader.status.ready_parent();
            return Ok(None);
        }

        let mut name = [0; wire::MAX_NAME_LEN + 1];
        let name = try_or_poison!(
            self.reader,
            read::bytes_buf(self.reader.inner, &mut name, wire::MAX_NAME_LEN)
        );

        if name.is_empty()
            || name.contains(&0)
            || name.contains(&b'/')
            || name == b"."
            || name == b".."
        {
            self.reader.status.poison();
            return Err(InvalidData.into());
        }

        // Enforce strict monotonicity of directory entry names.
        if &self.prev_name[..] >= name {
            self.reader.status.poison();
            return Err(InvalidData.into());
        }

        self.prev_name.clear();
        self.prev_name.extend_from_slice(name);

        try_or_poison!(self.reader, read::token(self.reader.inner, &wire::TOK_NOD));

        Ok(Some(Entry {
            name: &self.prev_name,
            // Don't need to worry about poisoning here: Node::new will do it for us if needed
            node: Node::new(self.reader.child())?,
        }))
    }
}

/// We use a stack of statuses to:
///   * Share poisoned state across all objects from the same underlying reader,
///     so we can check they are abandoned when an error occurs
///   * Make sure only the most recently created object is read from, and is fully exhausted
///     before anything it was created from is used again.
enum ArchiveReaderStatus<'a> {
    #[cfg(not(debug_assertions))]
    None(PhantomData<&'a ()>),
    #[cfg(debug_assertions)]
    StackTop { poisoned: bool, ready: bool },
    #[cfg(debug_assertions)]
    StackChild {
        poisoned: &'a mut bool,
        parent_ready: &'a mut bool,
        ready: bool,
    },
}

impl ArchiveReaderStatus<'_> {
    fn top() -> Self {
        #[cfg(debug_assertions)]
        {
            ArchiveReaderStatus::StackTop {
                poisoned: false,
                ready: true,
            }
        }

        #[cfg(not(debug_assertions))]
        ArchiveReaderStatus::None(PhantomData)
    }

    /// Poison all the objects sharing the same reader, to be used when an error occurs
    fn poison(&mut self) {
        match self {
            #[cfg(not(debug_assertions))]
            ArchiveReaderStatus::None(_) => {}
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackTop { poisoned: x, .. } => *x = true,
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackChild { poisoned: x, .. } => **x = true,
        }
    }

    /// Mark the parent as ready, allowing it to be used again and preventing this reference to the reader being used again.
    fn ready_parent(&mut self) {
        match self {
            #[cfg(not(debug_assertions))]
            ArchiveReaderStatus::None(_) => {}
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackTop { ready, .. } => {
                *ready = false;
            }
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackChild {
                ready,
                parent_ready,
                ..
            } => {
                *ready = false;
                **parent_ready = true;
            }
        };
    }

    fn poisoned(&self) -> bool {
        match self {
            #[cfg(not(debug_assertions))]
            ArchiveReaderStatus::None(_) => false,
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackTop { poisoned, .. } => *poisoned,
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackChild { poisoned, .. } => **poisoned,
        }
    }

    fn ready(&self) -> bool {
        match self {
            #[cfg(not(debug_assertions))]
            ArchiveReaderStatus::None(_) => true,
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackTop { ready, .. } => *ready,
            #[cfg(debug_assertions)]
            ArchiveReaderStatus::StackChild { ready, .. } => *ready,
        }
    }
}

impl<'a, 'r> ArchiveReader<'a, 'r> {
    /// Create a new child reader from this one.
    /// In debug mode, this reader will panic if called before the new child is exhausted / calls `ready_parent`
    fn child(&mut self) -> ArchiveReader<'_, 'r> {
        ArchiveReader {
            inner: self.inner,
            #[cfg(not(debug_assertions))]
            status: ArchiveReaderStatus::None(PhantomData),
            #[cfg(debug_assertions)]
            status: match &mut self.status {
                ArchiveReaderStatus::StackTop { poisoned, ready } => {
                    *ready = false;
                    ArchiveReaderStatus::StackChild {
                        poisoned,
                        parent_ready: ready,
                        ready: true,
                    }
                }
                ArchiveReaderStatus::StackChild {
                    poisoned, ready, ..
                } => {
                    *ready = false;
                    ArchiveReaderStatus::StackChild {
                        poisoned,
                        parent_ready: ready,
                        ready: true,
                    }
                }
            },
        }
    }

    /// Check the reader is in the correct status.
    /// Only does anything when debug assertions are on.
    #[inline(always)]
    fn check_correct(&self) {
        assert!(
            !self.status.poisoned(),
            "Archive reader used after it was meant to be abandoned!"
        );
        assert!(
            self.status.ready(),
            "Non-ready archive reader used! (Should've been reading from something else)"
        );
    }
}
