use std::{
    mem::MaybeUninit,
    pin::Pin,
    task::{self, Poll},
};

use tokio::io::{self, AsyncBufRead, AsyncRead, ErrorKind::InvalidData};

// Required reading for understanding this module.
use crate::{
    nar::{self, wire::PadPar},
    wire::{self, BytesReader},
};

mod read;
#[cfg(test)]
mod test;

pub type Reader<'a> = dyn AsyncBufRead + Unpin + Send + 'a;

/// Start reading a NAR file from `reader`.
pub async fn open<'a, 'r>(reader: &'a mut Reader<'r>) -> io::Result<Node<'a, 'r>> {
    read::token(reader, &nar::wire::TOK_NAR).await?;
    Node::new(reader).await
}

pub enum Node<'a, 'r: 'a> {
    Symlink {
        target: Vec<u8>,
    },
    File {
        executable: bool,
        reader: FileReader<'a, 'r>,
    },
    Directory(DirReader<'a, 'r>),
}

impl<'a, 'r: 'a> Node<'a, 'r> {
    /// Start reading a [Node], matching the next [wire::Node].
    ///
    /// Reading the terminating [wire::TOK_PAR] is done immediately for [Node::Symlink],
    /// but is otherwise left to [DirReader] or [BytesReader].
    async fn new(reader: &'a mut Reader<'r>) -> io::Result<Self> {
        Ok(match read::tag(reader).await? {
            nar::wire::Node::Sym => {
                let target = wire::read_bytes(reader, 1..=nar::wire::MAX_TARGET_LEN).await?;

                if target.contains(&0) {
                    return Err(InvalidData.into());
                }

                read::token(reader, &nar::wire::TOK_PAR).await?;

                Node::Symlink { target }
            }
            tag @ (nar::wire::Node::Reg | nar::wire::Node::Exe) => Node::File {
                executable: tag == nar::wire::Node::Exe,
                reader: FileReader {
                    inner: BytesReader::new_internal(reader, ..).await?,
                },
            },
            nar::wire::Node::Dir => Node::Directory(DirReader::new(reader)),
        })
    }
}

/// File contents, readable through the [AsyncRead] trait.
///
/// It comes with some caveats:
///  * You must always read the entire file, unless you intend to abandon the entire archive reader.
///  * You must abandon the entire archive reader upon the first error.
///
/// It's fine to read exactly `reader.len()` bytes without ever seeing an explicit EOF.
pub struct FileReader<'a, 'r> {
    inner: BytesReader<&'a mut Reader<'r>, PadPar>,
}

impl<'a, 'r> FileReader<'a, 'r> {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> u64 {
        self.inner.len()
    }
}

impl<'a, 'r> AsyncRead for FileReader<'a, 'r> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut task::Context,
        buf: &mut io::ReadBuf,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<'a, 'r> AsyncBufRead for FileReader<'a, 'r> {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut task::Context) -> Poll<io::Result<&[u8]>> {
        Pin::new(&mut self.get_mut().inner).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        Pin::new(&mut self.get_mut().inner).consume(amt)
    }
}

/// A directory iterator, yielding a sequence of [Node]s.
/// It must be fully consumed before reading further from the [DirReader] that produced it, if any.
pub struct DirReader<'a, 'r> {
    reader: &'a mut Reader<'r>,
    /// Previous directory entry name.
    /// We have to hang onto this to enforce name monotonicity.
    prev_name: Vec<u8>,
}

pub struct Entry<'a, 'r> {
    pub name: &'a [u8],
    pub node: Node<'a, 'r>,
}

impl<'a, 'r> DirReader<'a, 'r> {
    fn new(reader: &'a mut Reader<'r>) -> Self {
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
    pub async fn next(&mut self) -> io::Result<Option<Entry<'_, 'r>>> {
        // COME FROM the previous iteration: if we've already read an entry,
        // read its terminating TOK_PAR here.
        if !self.prev_name.is_empty() {
            read::token(self.reader, &nar::wire::TOK_PAR).await?;
        }

        if let nar::wire::Entry::None = read::tag(self.reader).await? {
            return Ok(None);
        }

        let mut name = [MaybeUninit::uninit(); nar::wire::MAX_NAME_LEN + 1];
        let name =
            wire::read_bytes_buf(self.reader, &mut name, 1..=nar::wire::MAX_NAME_LEN).await?;

        if name.contains(&0) || name.contains(&b'/') || name == b"." || name == b".." {
            return Err(InvalidData.into());
        }

        // Enforce strict monotonicity of directory entry names.
        if &self.prev_name[..] >= name {
            return Err(InvalidData.into());
        }

        self.prev_name.clear();
        self.prev_name.extend_from_slice(name);

        read::token(self.reader, &nar::wire::TOK_NOD).await?;

        Ok(Some(Entry {
            name: &self.prev_name,
            node: Node::new(self.reader).await?,
        }))
    }
}
