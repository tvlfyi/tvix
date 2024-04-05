use std::{
    io::{Error, ErrorKind},
    ops::RangeBounds,
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::primitive;

#[allow(dead_code)]
/// Read a limited number of bytes from the AsyncRead.
/// Rejects reading more than `allowed_size` bytes of payload.
/// Internally takes care of dealing with the padding, so the returned `Vec<u8>`
/// only contains the payload.
/// This always buffers the entire contents into memory, we'll add a streaming
/// version later.
pub async fn read_bytes<R, S>(r: &mut R, allowed_size: S) -> std::io::Result<Vec<u8>>
where
    R: AsyncReadExt + Unpin,
    S: RangeBounds<u64>,
{
    // read the length field
    let len = primitive::read_u64(r).await?;

    if !allowed_size.contains(&len) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "signalled package size not in allowed range",
        ));
    }

    // calculate the total length, including padding.
    // byte packets are padded to 8 byte blocks each.
    let padded_len = padding_len(len) as u64 + (len as u64);
    let mut limited_reader = r.take(padded_len);

    let mut buf = Vec::new();

    let s = limited_reader.read_to_end(&mut buf).await?;

    // make sure we got exactly the number of bytes, and not less.
    if s as u64 != padded_len {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "got less bytes than expected",
        ));
    }

    let (_content, padding) = buf.split_at(len as usize);

    // ensure the padding is all zeroes.
    if !padding.iter().all(|e| *e == b'\0') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "padding is not all zeroes",
        ));
    }

    // return the data without the padding
    buf.truncate(len as usize);
    Ok(buf)
}

/// Read a Nix daemon string from the AsyncWrite, encoded as utf8.
/// Rejects reading more than `allowed_size` bytes
///
/// A Nix daemon string is made up of two distincts parts:
/// 1. Its lenght, LE-encoded on 64 bits.
/// 2. Its content. 0-padded on 64 bits.
pub async fn read_string<R, S>(r: &mut R, allowed_size: S) -> std::io::Result<String>
where
    R: AsyncReadExt + Unpin,
    S: RangeBounds<u64>,
{
    let bytes = read_bytes(r, allowed_size).await?;
    String::from_utf8(bytes).map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

/// Writes a sequence of sized bits to a (hopefully buffered)
/// [AsyncWriteExt] handle.
///
/// On the wire, it looks as follows:
///
/// 1. Number of bytes contained in the buffer we're about to write on
///    the wire. (LE-encoded on 64 bits)
/// 2. Raw payload.
/// 3. Null padding up until the next 8 bytes alignment block.
///
/// Note: if performance matters to you, make sure your
/// [AsyncWriteExt] handle is buffered. This function is quite
/// write-intesive.
pub async fn write_bytes<W: AsyncWriteExt + Unpin>(w: &mut W, b: &[u8]) -> std::io::Result<()> {
    // We're assuming the handle is buffered: we can afford not
    // writing all the bytes in one go.
    let len = b.len();
    primitive::write_u64(w, len as u64).await?;
    w.write_all(b).await?;
    let padding = padding_len(len as u64);
    if padding != 0 {
        w.write_all(&vec![0; padding as usize]).await?;
    }
    Ok(())
}

#[allow(dead_code)]
/// Read an unlimited number of bytes from the AsyncRead.
/// Note this can exhaust memory.
/// Internally uses [read_bytes], which takes care of dealing with the padding,
/// so the returned `Vec<u8>` only contains the payload.
pub async fn read_bytes_unchecked<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    read_bytes(r, 0u64..).await
}

/// Computes the number of bytes we should add to len (a length in
/// bytes) to be alined on 64 bits (8 bytes).
pub(crate) fn padding_len(len: u64) -> u8 {
    let modulo = len % 8;
    if modulo == 0 {
        0
    } else {
        8 - modulo as u8
    }
}

#[cfg(test)]
mod tests {
    use tokio_test::{assert_ok, io::Builder};

    use super::*;
    use hex_literal::hex;

    #[tokio::test]
    async fn test_read_8_bytes_unchecked() {
        let mut mock = Builder::new()
            .read(&8u64.to_le_bytes())
            .read(&12345678u64.to_le_bytes())
            .build();

        assert_eq!(
            &12345678u64.to_le_bytes(),
            read_bytes_unchecked(&mut mock).await.unwrap().as_slice()
        );
    }

    #[tokio::test]
    async fn test_read_9_bytes_unchecked() {
        let mut mock = Builder::new()
            .read(&9u64.to_le_bytes())
            .read(&hex!("01020304050607080900000000000000"))
            .build();

        assert_eq!(
            hex!("010203040506070809"),
            read_bytes_unchecked(&mut mock).await.unwrap().as_slice()
        );
    }

    #[tokio::test]
    async fn test_read_0_bytes_unchecked() {
        // A empty byte packet is essentially just the 0 length field.
        // No data is read, and there's zero padding.
        let mut mock = Builder::new().read(&0u64.to_le_bytes()).build();

        assert_eq!(
            hex!(""),
            read_bytes_unchecked(&mut mock).await.unwrap().as_slice()
        );
    }

    #[tokio::test]
    /// Ensure we don't read any further than the size field if the length
    /// doesn't match the range we want to accept.
    async fn test_read_reject_too_large() {
        let mut mock = Builder::new().read(&100u64.to_le_bytes()).build();

        read_bytes(&mut mock, 10..10)
            .await
            .expect_err("expect this to fail");
    }

    #[tokio::test]
    async fn test_write_bytes_no_padding() {
        let input = hex!("6478696f34657661");
        let len = input.len() as u64;
        let mut mock = Builder::new()
            .write(&len.to_le_bytes())
            .write(&input)
            .build();
        assert_ok!(write_bytes(&mut mock, &input).await)
    }
    #[tokio::test]
    async fn test_write_bytes_with_padding() {
        let input = hex!("322e332e3137");
        let len = input.len() as u64;
        let mut mock = Builder::new()
            .write(&len.to_le_bytes())
            .write(&hex!("322e332e31370000"))
            .build();
        assert_ok!(write_bytes(&mut mock, &input).await)
    }
}
