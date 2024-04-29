use tokio::io::{
    self, AsyncReadExt,
    ErrorKind::{InvalidData, UnexpectedEof},
};

use crate::nar::wire::Tag;

use super::Reader;

/// Consume a known token from the reader.
pub async fn token<const N: usize>(reader: &mut Reader<'_>, token: &[u8; N]) -> io::Result<()> {
    let mut buf = [0u8; N];

    // This implements something similar to [AsyncReadExt::read_exact], but verifies that
    // the input data matches the token while we read it. These two slices respectively
    // represent the remaining token to be verified, and the remaining input buffer.
    let mut token = &token[..];
    let mut buf = &mut buf[..];

    while !token.is_empty() {
        match reader.read(buf).await? {
            0 => {
                return Err(UnexpectedEof.into());
            }
            n => {
                let (t, b);
                (t, token) = token.split_at(n);
                (b, buf) = buf.split_at_mut(n);

                if t != b {
                    return Err(InvalidData.into());
                }
            }
        }
    }

    Ok(())
}

/// Consume a [Tag] from the reader.
pub async fn tag<T: Tag>(reader: &mut Reader<'_>) -> io::Result<T> {
    let mut buf = T::make_buf();
    let buf = buf.as_mut();

    // first read the known minimum length…
    reader.read_exact(&mut buf[..T::MIN]).await?;

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
        reader.read_exact(&mut buf[..rest]).await?;

        // and make sure it's what we expect
        if buf[..rest] != *tail {
            return Err(InvalidData.into());
        }
    }

    Ok(tag)
}
