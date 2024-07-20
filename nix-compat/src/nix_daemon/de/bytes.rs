use bytes::Bytes;

use super::{Error, NixDeserialize, NixRead};

impl NixDeserialize for Bytes {
    async fn try_deserialize<R>(reader: &mut R) -> Result<Option<Self>, R::Error>
    where
        R: ?Sized + NixRead + Send,
    {
        reader.try_read_bytes().await
    }
}

impl NixDeserialize for String {
    async fn try_deserialize<R>(reader: &mut R) -> Result<Option<Self>, R::Error>
    where
        R: ?Sized + NixRead + Send,
    {
        if let Some(buf) = reader.try_read_bytes().await? {
            String::from_utf8(buf.to_vec())
                .map_err(R::Error::invalid_data)
                .map(Some)
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod test {
    use std::io;

    use hex_literal::hex;
    use rstest::rstest;
    use tokio_test::io::Builder;

    use crate::nix_daemon::de::{NixRead, NixReader};

    #[rstest]
    #[case::empty("", &hex!("0000 0000 0000 0000"))]
    #[case::one(")", &hex!("0100 0000 0000 0000 2900 0000 0000 0000"))]
    #[case::two("it", &hex!("0200 0000 0000 0000 6974 0000 0000 0000"))]
    #[case::three("tea", &hex!("0300 0000 0000 0000 7465 6100 0000 0000"))]
    #[case::four("were", &hex!("0400 0000 0000 0000 7765 7265 0000 0000"))]
    #[case::five("where", &hex!("0500 0000 0000 0000 7768 6572 6500 0000"))]
    #[case::six("unwrap", &hex!("0600 0000 0000 0000 756E 7772 6170 0000"))]
    #[case::seven("where's", &hex!("0700 0000 0000 0000 7768 6572 6527 7300"))]
    #[case::aligned("read_tea", &hex!("0800 0000 0000 0000 7265 6164 5F74 6561"))]
    #[case::more_bytes("read_tess", &hex!("0900 0000 0000 0000 7265 6164 5F74 6573 7300 0000 0000 0000"))]
    #[case::utf8("The quick brown 🦊 jumps over 13 lazy 🐶.", &hex!("2D00 0000 0000 0000  5468 6520 7175 6963  6b20 6272 6f77 6e20  f09f a68a 206a 756d  7073 206f 7665 7220  3133 206c 617a 7920  f09f 90b6 2e00 0000"))]
    #[tokio::test]
    async fn test_read_string(#[case] expected: &str, #[case] data: &[u8]) {
        let mock = Builder::new().read(data).build();
        let mut reader = NixReader::new(mock);
        let actual: String = reader.read_value().await.unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn test_read_string_invalid() {
        let mock = Builder::new()
            .read(&hex!("0300 0000 0000 0000 EDA0 8000 0000 0000"))
            .build();
        let mut reader = NixReader::new(mock);
        assert_eq!(
            io::ErrorKind::InvalidData,
            reader.read_value::<String>().await.unwrap_err().kind()
        );
    }
}
