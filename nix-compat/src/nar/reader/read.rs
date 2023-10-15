//! Helpers for reading [crate::nar::wire] format.

use std::io::{
    self,
    ErrorKind::{Interrupted, InvalidData, UnexpectedEof},
};

use super::Reader;
use crate::nar::wire::Tag;

/// Consume a little-endian [u64] from the reader.
pub fn u64(reader: &mut Reader) -> io::Result<u64> {
    let mut buf = [0; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Consume a byte string of up to `max_len` bytes from the reader.
pub fn bytes(reader: &mut Reader, max_len: usize) -> io::Result<Vec<u8>> {
    assert!(max_len <= isize::MAX as usize);

    // read the length, and reject excessively large values
    let len = self::u64(reader)?;
    if len > max_len as u64 {
        return Err(InvalidData.into());
    }
    // we know the length fits in a usize now
    let len = len as usize;

    // read the data and padding into a buffer
    let buf_len = (len + 7) & !7;
    let mut buf = vec![0; buf_len];
    reader.read_exact(&mut buf)?;

    // verify that the padding is all zeroes
    for b in buf.drain(len..) {
        if b != 0 {
            return Err(InvalidData.into());
        }
    }

    Ok(buf)
}

/// Consume a known token from the reader.
pub fn token<const N: usize>(reader: &mut Reader, token: &[u8; N]) -> io::Result<()> {
    let mut buf = [0u8; N];

    // This implements something similar to [Read::read_exact], but verifies that
    // the input data matches the token while we read it. These two slices respectively
    // represent the remaining token to be verified, and the remaining input buffer.
    let mut token = &token[..];
    let mut buf = &mut buf[..];

    while !token.is_empty() {
        match reader.read(buf) {
            Ok(0) => {
                return Err(UnexpectedEof.into());
            }
            Ok(n) => {
                let (t, b);
                (t, token) = token.split_at(n);
                (b, buf) = buf.split_at_mut(n);

                if t != b {
                    return Err(InvalidData.into());
                }
            }
            Err(e) => {
                if e.kind() != Interrupted {
                    return Err(e);
                }
            }
        }
    }

    Ok(())
}

/// Consume a [Tag] from the reader.
pub fn tag<T: Tag>(reader: &mut Reader) -> io::Result<T> {
    let mut buf = T::make_buf();
    let buf = buf.as_mut();

    // first read the known minimum length…
    reader.read_exact(&mut buf[..T::MIN])?;

    // then decide which tag we're expecting
    let tag = T::from_u8(buf[T::OFF]).ok_or(InvalidData)?;
    let (head, tail) = tag.as_bytes().split_at(T::MIN);

    // make sure what we've read so far is valid
    if buf[..T::MIN] != *head {
        return Err(InvalidData.into());
    }

    // …then read the rest, if any
    if !tail.is_empty() {
        let rest = tail.len();
        reader.read_exact(&mut buf[..rest])?;

        // and make sure it's what we expect
        if buf[..rest] != *tail {
            return Err(InvalidData.into());
        }
    }

    Ok(tag)
}
