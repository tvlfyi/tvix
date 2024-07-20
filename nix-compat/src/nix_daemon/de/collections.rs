use std::{collections::BTreeMap, future::Future};

use super::{NixDeserialize, NixRead};

#[allow(clippy::manual_async_fn)]
impl<T> NixDeserialize for Vec<T>
where
    T: NixDeserialize + Send,
{
    fn try_deserialize<R>(
        reader: &mut R,
    ) -> impl Future<Output = Result<Option<Self>, R::Error>> + Send + '_
    where
        R: ?Sized + NixRead + Send,
    {
        async move {
            if let Some(len) = reader.try_read_value::<usize>().await? {
                let mut ret = Vec::with_capacity(len);
                for _ in 0..len {
                    ret.push(reader.read_value().await?);
                }
                Ok(Some(ret))
            } else {
                Ok(None)
            }
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl<K, V> NixDeserialize for BTreeMap<K, V>
where
    K: NixDeserialize + Ord + Send,
    V: NixDeserialize + Send,
{
    fn try_deserialize<R>(
        reader: &mut R,
    ) -> impl Future<Output = Result<Option<Self>, R::Error>> + Send + '_
    where
        R: ?Sized + NixRead + Send,
    {
        async move {
            if let Some(len) = reader.try_read_value::<usize>().await? {
                let mut ret = BTreeMap::new();
                for _ in 0..len {
                    let key = reader.read_value().await?;
                    let value = reader.read_value().await?;
                    ret.insert(key, value);
                }
                Ok(Some(ret))
            } else {
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::fmt;

    use hex_literal::hex;
    use rstest::rstest;
    use tokio_test::io::Builder;

    use crate::nix_daemon::de::{NixDeserialize, NixRead, NixReader};

    #[rstest]
    #[case::empty(vec![], &hex!("0000 0000 0000 0000"))]
    #[case::one(vec![0x29], &hex!("0100 0000 0000 0000 2900 0000 0000 0000"))]
    #[case::two(vec![0x7469, 10], &hex!("0200 0000 0000 0000 6974 0000 0000 0000 0A00 0000 0000 0000"))]
    #[tokio::test]
    async fn test_read_small_vec(#[case] expected: Vec<usize>, #[case] data: &[u8]) {
        let mock = Builder::new().read(data).build();
        let mut reader = NixReader::new(mock);
        let actual: Vec<usize> = reader.read_value().await.unwrap();
        assert_eq!(actual, expected);
    }

    fn empty_map() -> BTreeMap<usize, u64> {
        BTreeMap::new()
    }
    macro_rules! map {
        ($($key:expr => $value:expr),*) => {{
            let mut ret = BTreeMap::new();
            $(ret.insert($key, $value);)*
            ret
        }};
    }

    #[rstest]
    #[case::empty(empty_map(), &hex!("0000 0000 0000 0000"))]
    #[case::one(map![0x7469usize => 10u64], &hex!("0100 0000 0000 0000 6974 0000 0000 0000 0A00 0000 0000 0000"))]
    #[tokio::test]
    async fn test_read_small_btree_map<E>(#[case] expected: E, #[case] data: &[u8])
    where
        E: NixDeserialize + PartialEq + fmt::Debug,
    {
        let mock = Builder::new().read(data).build();
        let mut reader = NixReader::new(mock);
        let actual: E = reader.read_value().await.unwrap();
        assert_eq!(actual, expected);
    }
}
