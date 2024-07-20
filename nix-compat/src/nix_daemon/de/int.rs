use super::{Error, NixDeserialize, NixRead};

impl NixDeserialize for u64 {
    async fn try_deserialize<R>(reader: &mut R) -> Result<Option<Self>, R::Error>
    where
        R: ?Sized + NixRead + Send,
    {
        reader.try_read_number().await
    }
}

impl NixDeserialize for usize {
    async fn try_deserialize<R>(reader: &mut R) -> Result<Option<Self>, R::Error>
    where
        R: ?Sized + NixRead + Send,
    {
        if let Some(value) = reader.try_read_number().await? {
            value.try_into().map_err(R::Error::invalid_data).map(Some)
        } else {
            Ok(None)
        }
    }
}

impl NixDeserialize for bool {
    async fn try_deserialize<R>(reader: &mut R) -> Result<Option<Self>, R::Error>
    where
        R: ?Sized + NixRead + Send,
    {
        Ok(reader.try_read_number().await?.map(|v| v != 0))
    }
}
impl NixDeserialize for i64 {
    async fn try_deserialize<R>(reader: &mut R) -> Result<Option<Self>, R::Error>
    where
        R: ?Sized + NixRead + Send,
    {
        Ok(reader.try_read_number().await?.map(|v| v as i64))
    }
}

#[cfg(test)]
mod test {
    use hex_literal::hex;
    use rstest::rstest;
    use tokio_test::io::Builder;

    use crate::nix_daemon::de::{NixRead, NixReader};

    #[rstest]
    #[case::simple_false(false, &hex!("0000 0000 0000 0000"))]
    #[case::simple_true(true, &hex!("0100 0000 0000 0000"))]
    #[case::other_true(true, &hex!("1234 5600 0000 0000"))]
    #[case::max_true(true, &hex!("FFFF FFFF FFFF FFFF"))]
    #[tokio::test]
    async fn test_read_bool(#[case] expected: bool, #[case] data: &[u8]) {
        let mock = Builder::new().read(data).build();
        let mut reader = NixReader::new(mock);
        let actual: bool = reader.read_value().await.unwrap();
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::zero(0, &hex!("0000 0000 0000 0000"))]
    #[case::one(1, &hex!("0100 0000 0000 0000"))]
    #[case::other(0x563412, &hex!("1234 5600 0000 0000"))]
    #[case::max_value(u64::MAX, &hex!("FFFF FFFF FFFF FFFF"))]
    #[tokio::test]
    async fn test_read_u64(#[case] expected: u64, #[case] data: &[u8]) {
        let mock = Builder::new().read(data).build();
        let mut reader = NixReader::new(mock);
        let actual: u64 = reader.read_value().await.unwrap();
        assert_eq!(actual, expected);
    }

    #[rstest]
    #[case::zero(0, &hex!("0000 0000 0000 0000"))]
    #[case::one(1, &hex!("0100 0000 0000 0000"))]
    #[case::other(0x563412, &hex!("1234 5600 0000 0000"))]
    #[case::max_value(usize::MAX, &usize::MAX.to_le_bytes())]
    #[tokio::test]
    async fn test_read_usize(#[case] expected: usize, #[case] data: &[u8]) {
        let mock = Builder::new().read(data).build();
        let mut reader = NixReader::new(mock);
        let actual: usize = reader.read_value().await.unwrap();
        assert_eq!(actual, expected);
    }

    // FUTUREWORK: Test this on supported hardware
    #[tokio::test]
    #[cfg(any(target_pointer_width = "16", target_pointer_width = "32"))]
    async fn test_read_usize_overflow() {
        let mock = Builder::new().read(&u64::MAX.to_le_bytes()).build();
        let mut reader = NixReader::new(mock);
        assert_eq!(
            std::io::ErrorKind::InvalidData,
            reader.read_value::<usize>().await.unwrap_err().kind()
        );
    }
}
