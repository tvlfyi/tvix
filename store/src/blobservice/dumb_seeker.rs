use std::io;

use tracing::{debug, instrument};

use super::BlobReader;

/// This implements [io::Seek] for and [io::Read] by simply skipping over some
/// bytes, keeping track of the position.
/// It fails whenever you try to seek backwards.
pub struct DumbSeeker<R: io::Read> {
    r: R,
    pos: u64,
}

impl<R: io::Read> DumbSeeker<R> {
    pub fn new(r: R) -> Self {
        DumbSeeker { r, pos: 0 }
    }
}

impl<R: io::Read> io::Read for DumbSeeker<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.r.read(buf)?;

        self.pos += bytes_read as u64;

        Ok(bytes_read)
    }
}

impl<R: io::Read> io::Seek for DumbSeeker<R> {
    #[instrument(skip(self))]
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        let absolute_offset: u64 = match pos {
            io::SeekFrom::Start(start_offset) => {
                if start_offset < self.pos {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!("can't seek backwards ({} -> {})", self.pos, start_offset),
                    ));
                } else {
                    start_offset
                }
            }
            // we don't know the total size, can't support this.
            io::SeekFrom::End(_end_offset) => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "can't seek from end",
                ));
            }
            io::SeekFrom::Current(relative_offset) => {
                if relative_offset < 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "can't seek backwards relative to current position",
                    ));
                } else {
                    self.pos + relative_offset as u64
                }
            }
        };

        debug!(absolute_offset=?absolute_offset, "seek");

        // we already know absolute_offset is larger than self.pos
        debug_assert!(
            absolute_offset >= self.pos,
            "absolute_offset {} is larger than self.pos {}",
            absolute_offset,
            self.pos
        );

        // calculate bytes to skip
        let bytes_to_skip: u64 = absolute_offset - self.pos;

        // discard these bytes. We can't use take() as it requires ownership of
        // self.r, but we only have &mut self.
        let mut buf = [0; 1024];
        let mut bytes_skipped: u64 = 0;
        while bytes_skipped < bytes_to_skip {
            let len = std::cmp::min(bytes_to_skip - bytes_skipped, buf.len() as u64);
            match self.r.read(&mut buf[..len as usize]) {
                Ok(0) => break,
                Ok(n) => bytes_skipped += n as u64,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }

        // This will fail when seeking past the end of self.r
        if bytes_to_skip != bytes_skipped {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "tried to skip {} bytes, but only was able to skip {} until reaching EOF",
                    bytes_to_skip, bytes_skipped
                ),
            ));
        }

        self.pos = absolute_offset;

        // return the new position from the start of the stream
        Ok(absolute_offset)
    }
}

/// A Cursor<Vec<u8>> can be used as a BlobReader.
impl<R: io::Read + Send + 'static> BlobReader for DumbSeeker<R> {}
