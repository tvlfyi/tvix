// SPDX-FileCopyrightText: 2023 embr <git@liclac.eu>
//
// SPDX-License-Identifier: EUPL-1.2

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

// LE-encoded nixc on 64 bits. Because why not.
pub static MAGIC_HELLO: [u8; 8] = *b"cxin\0\0\0\0";
// LE-encoded dxio on 64 bits. What's dxio? I have no clue.
pub static MAGIC_HELLO_RESPONSE: [u8; 8] = *b"oixd\0\0\0\0";
// LE-encoded protocol version.
pub static PROTOCOL_VERSION: [u8; 8] = [0x23, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Read a LE u32 from the least-significant bytes of a LE u64.
///
/// Overall, it looks like this on the wire:
///
/// 00 0x12 0x32 0x00 0x00 0x00 0x00 0x00 0x00
///    |------------------|-------------------|
///          LE u32            padding
///
/// Not sure why the protocol does this instead of using a plain u64,
/// but well, it is what it is.
///
/// Analogous to the readInt function in cppnix.
pub async fn read_u32<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<u32> {
    let val64 = r.read_u64_le().await?;
    u32::try_from(val64).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "padding is not all zeroes")
    })
}

#[allow(dead_code)]
/// Read a u64 from the AsyncRead (little endian).
pub async fn read_u64<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<u64> {
    r.read_u64_le().await
}

/// Write a u64 to the AsyncWrite (little endian).
pub async fn write_u64<W: AsyncWrite + Unpin>(w: &mut W, v: u64) -> std::io::Result<()> {
    w.write_u64_le(v).await
}

#[allow(dead_code)]
/// Read a boolean from the AsyncRead, encoded as u64 (>0 is true).
pub async fn read_bool<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<bool> {
    Ok(read_u64(r).await? > 0)
}

#[allow(dead_code)]
/// Write a boolean to the AsyncWrite, encoded as u64 (>0 is true).
pub async fn write_bool<W: AsyncWrite + Unpin>(w: &mut W, v: bool) -> std::io::Result<()> {
    write_u64(w, if v { 1u64 } else { 0u64 }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;
    use tokio_test::{assert_err, io::Builder};

    // Integers.
    #[tokio::test]
    async fn test_read_u64() {
        let mut mock = Builder::new().read(&1234567890u64.to_le_bytes()).build();
        assert_eq!(1234567890u64, read_u64(&mut mock).await.unwrap());
    }
    #[tokio::test]
    async fn test_write_u64() {
        let mut mock = Builder::new().write(&1234567890u64.to_le_bytes()).build();
        write_u64(&mut mock, 1234567890).await.unwrap();
    }

    // Booleans.
    #[tokio::test]
    async fn test_read_bool_0() {
        let mut mock = Builder::new().read(&0u64.to_le_bytes()).build();
        assert!(!read_bool(&mut mock).await.unwrap());
    }
    #[tokio::test]
    async fn test_read_bool_1() {
        let mut mock = Builder::new().read(&1u64.to_le_bytes()).build();
        assert!(read_bool(&mut mock).await.unwrap());
    }
    #[tokio::test]
    async fn test_read_bool_2() {
        let mut mock = Builder::new().read(&2u64.to_le_bytes()).build();
        assert!(read_bool(&mut mock).await.unwrap());
    }

    #[tokio::test]
    async fn test_write_bool_false() {
        let mut mock = Builder::new().write(&0u64.to_le_bytes()).build();
        write_bool(&mut mock, false).await.unwrap();
    }
    #[tokio::test]
    async fn test_write_bool_true() {
        let mut mock = Builder::new().write(&1u64.to_le_bytes()).build();
        write_bool(&mut mock, true).await.unwrap();
    }
    #[tokio::test]
    async fn test_read_u32() {
        let mut mock = Builder::new().read(&hex!("7856341200000000")).build();
        let res = read_u32(&mut mock).await.unwrap();
        assert_eq!(res, 0x12345678);
    }
    #[tokio::test]
    async fn test_read_too_large_u32_fail() {
        let mut mock = Builder::new().read(&hex!("7856341298760000")).build();
        let res = read_u32(&mut mock).await;
        assert_err!(res);
    }
}
