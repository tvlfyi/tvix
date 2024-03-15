use std::ops::RangeBounds;

use tokio::io::AsyncReadExt;

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
    let padded_len = if len % 8 == 0 {
        len
    } else {
        len + (8 - len % 8)
    };

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

#[allow(dead_code)]
/// Read an unlimited number of bytes from the AsyncRead.
/// Note this can exhaust memory.
/// Internally uses [read_bytes], which takes care of dealing with the padding,
/// so the returned `Vec<u8>` only contains the payload.
pub async fn read_bytes_unchecked<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    read_bytes(r, 0u64..).await
}

#[cfg(test)]
mod tests {
    use tokio_test::io::Builder;

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
    async fn test_reject_too_large() {
        let mut mock = Builder::new().read(&100u64.to_le_bytes()).build();

        read_bytes(&mut mock, 10..10)
            .await
            .expect_err("expect this to fail");
    }
}
