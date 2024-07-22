use std::str::FromStr;

use nix_compat::nix_daemon::de::mock::{Builder, Error};
use nix_compat::nix_daemon::de::NixRead;
use nix_compat_derive::NixDeserialize;

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
pub struct UnitTest;

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
pub struct EmptyTupleTest();

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
pub struct StructTest {
    first: u64,
    second: String,
}

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
pub struct TupleTest(u64, String);

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
pub struct StructVersionTest {
    test: u64,
    #[nix(version = "20..")]
    hello: String,
}

fn default_test() -> StructVersionTest {
    StructVersionTest {
        test: 89,
        hello: String::from("klomp"),
    }
}

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
pub struct TupleVersionTest(u64, #[nix(version = "25..")] String);

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
pub struct TupleVersionDefaultTest(
    u64,
    #[nix(version = "..25", default = "default_test")] StructVersionTest,
);

#[tokio::test]
async fn read_unit() {
    let mut mock = Builder::new().build();
    let v: UnitTest = mock.read_value().await.unwrap();
    assert_eq!(UnitTest, v);
}

#[tokio::test]
async fn read_empty_tuple() {
    let mut mock = Builder::new().build();
    let v: EmptyTupleTest = mock.read_value().await.unwrap();
    assert_eq!(EmptyTupleTest(), v);
}

#[tokio::test]
async fn read_struct() {
    let mut mock = Builder::new().read_number(89).read_slice(b"klomp").build();
    let v: StructTest = mock.read_value().await.unwrap();
    assert_eq!(
        StructTest {
            first: 89,
            second: String::from("klomp"),
        },
        v
    );
}

#[tokio::test]
async fn read_tuple() {
    let mut mock = Builder::new().read_number(89).read_slice(b"klomp").build();
    let v: TupleTest = mock.read_value().await.unwrap();
    assert_eq!(TupleTest(89, String::from("klomp")), v);
}

#[tokio::test]
async fn read_struct_version() {
    let mut mock = Builder::new()
        .version((1, 20))
        .read_number(89)
        .read_slice(b"klomp")
        .build();
    let v: StructVersionTest = mock.read_value().await.unwrap();
    assert_eq!(default_test(), v);
}

#[tokio::test]
async fn read_struct_without_version() {
    let mut mock = Builder::new().version((1, 19)).read_number(89).build();
    let v: StructVersionTest = mock.read_value().await.unwrap();
    assert_eq!(
        StructVersionTest {
            test: 89,
            hello: String::new(),
        },
        v
    );
}

#[tokio::test]
async fn read_tuple_version() {
    let mut mock = Builder::new()
        .version((1, 26))
        .read_number(89)
        .read_slice(b"klomp")
        .build();
    let v: TupleVersionTest = mock.read_value().await.unwrap();
    assert_eq!(TupleVersionTest(89, "klomp".into()), v);
}

#[tokio::test]
async fn read_tuple_without_version() {
    let mut mock = Builder::new().version((1, 19)).read_number(89).build();
    let v: TupleVersionTest = mock.read_value().await.unwrap();
    assert_eq!(TupleVersionTest(89, String::new()), v);
}

#[tokio::test]
async fn read_complex_1() {
    let mut mock = Builder::new()
        .version((1, 19))
        .read_number(999)
        .read_number(666)
        .build();
    let v: TupleVersionDefaultTest = mock.read_value().await.unwrap();
    assert_eq!(
        TupleVersionDefaultTest(
            999,
            StructVersionTest {
                test: 666,
                hello: String::new()
            }
        ),
        v
    );
}

#[tokio::test]
async fn read_complex_2() {
    let mut mock = Builder::new()
        .version((1, 20))
        .read_number(999)
        .read_number(666)
        .read_slice(b"The quick brown \xF0\x9F\xA6\x8A jumps over 13 lazy \xF0\x9F\x90\xB6.")
        .build();
    let v: TupleVersionDefaultTest = mock.read_value().await.unwrap();
    assert_eq!(
        TupleVersionDefaultTest(
            999,
            StructVersionTest {
                test: 666,
                hello: String::from("The quick brown 🦊 jumps over 13 lazy 🐶.")
            }
        ),
        v
    );
}

#[tokio::test]
async fn read_complex_3() {
    let mut mock = Builder::new().version((1, 25)).read_number(999).build();
    let v: TupleVersionDefaultTest = mock.read_value().await.unwrap();
    assert_eq!(
        TupleVersionDefaultTest(
            999,
            StructVersionTest {
                test: 89,
                hello: String::from("klomp")
            }
        ),
        v
    );
}

#[tokio::test]
async fn read_complex_4() {
    let mut mock = Builder::new().version((1, 26)).read_number(999).build();
    let v: TupleVersionDefaultTest = mock.read_value().await.unwrap();
    assert_eq!(
        TupleVersionDefaultTest(
            999,
            StructVersionTest {
                test: 89,
                hello: String::from("klomp")
            }
        ),
        v
    );
}

#[tokio::test]
async fn read_field_invalid_data() {
    let mut mock = Builder::new()
        .read_number(666)
        .read_slice(b"The quick brown \xED\xA0\x80 jumped.")
        .build();
    let err = mock.read_value::<StructTest>().await.unwrap_err();
    assert_eq!(
        Error::InvalidData("invalid utf-8 sequence of 1 bytes from index 16".into()),
        err
    );
}

#[tokio::test]
async fn read_field_missing_data() {
    let mut mock = Builder::new().read_number(666).build();
    let err = mock.read_value::<StructTest>().await.unwrap_err();
    assert_eq!(Error::MissingData("unexpected end-of-file".into()), err);
}

#[tokio::test]
async fn read_field_no_data() {
    let mut mock = Builder::new().build();
    let err = mock.read_value::<StructTest>().await.unwrap_err();
    assert_eq!(Error::MissingData("unexpected end-of-file".into()), err);
}

#[tokio::test]
async fn read_field_reader_error_first() {
    let mut mock = Builder::new()
        .read_number_error(Error::InvalidData("Bad reader".into()))
        .build();
    let err = mock.read_value::<StructTest>().await.unwrap_err();
    assert_eq!(Error::InvalidData("Bad reader".into()), err);
}

#[tokio::test]
async fn read_field_reader_error_later() {
    let mut mock = Builder::new()
        .read_number(999)
        .read_bytes_error(Error::InvalidData("Bad reader".into()))
        .build();
    let err = mock.read_value::<StructTest>().await.unwrap_err();
    assert_eq!(Error::InvalidData("Bad reader".into()), err);
}

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
#[nix(from_str)]
struct TestFromStr;

impl FromStr for TestFromStr {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "test" {
            Ok(TestFromStr)
        } else {
            Err(s.into())
        }
    }
}

#[tokio::test]
async fn read_from_str() {
    let mut mock = Builder::new().read_slice(b"test").build();
    let value = mock.read_value::<TestFromStr>().await.unwrap();
    assert_eq!(TestFromStr, value);
}

#[tokio::test]
async fn read_from_str_invalid_data() {
    let mut mock = Builder::new().read_slice(b"wrong string").build();
    let err = mock.read_value::<TestFromStr>().await.unwrap_err();
    assert_eq!(Error::InvalidData("wrong string".into()), err);
}

#[tokio::test]
async fn read_from_str_invalid_string() {
    let mut mock = Builder::new()
        .read_slice(b"The quick brown \xED\xA0\x80 jumped.")
        .build();
    let err = mock.read_value::<TestFromStr>().await.unwrap_err();
    assert_eq!(
        Error::InvalidData("invalid utf-8 sequence of 1 bytes from index 16".into()),
        err
    );
}

#[tokio::test]
async fn read_from_str_reader_error() {
    let mut mock = Builder::new()
        .read_bytes_error(Error::InvalidData("Bad reader".into()))
        .build();
    let err = mock.read_value::<TestFromStr>().await.unwrap_err();
    assert_eq!(Error::InvalidData("Bad reader".into()), err);
}

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
#[nix(try_from = "u64")]
struct TestTryFromU64;

impl TryFrom<u64> for TestTryFromU64 {
    type Error = u64;

    fn try_from(value: u64) -> Result<TestTryFromU64, Self::Error> {
        if value == 42 {
            Ok(TestTryFromU64)
        } else {
            Err(value)
        }
    }
}

#[tokio::test]
async fn read_try_from_u64() {
    let mut mock = Builder::new().read_number(42).build();
    let value = mock.read_value::<TestTryFromU64>().await.unwrap();
    assert_eq!(TestTryFromU64, value);
}

#[tokio::test]
async fn read_try_from_u64_invalid_data() {
    let mut mock = Builder::new().read_number(666).build();
    let err = mock.read_value::<TestTryFromU64>().await.unwrap_err();
    assert_eq!(Error::InvalidData("666".into()), err);
}

#[tokio::test]
async fn read_try_from_u64_reader_error() {
    let mut mock = Builder::new()
        .read_number_error(Error::InvalidData("Bad reader".into()))
        .build();
    let err = mock.read_value::<TestTryFromU64>().await.unwrap_err();
    assert_eq!(Error::InvalidData("Bad reader".into()), err);
}

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
#[nix(from = "u64")]
struct TestFromU64;

impl From<u64> for TestFromU64 {
    fn from(_value: u64) -> TestFromU64 {
        TestFromU64
    }
}

#[tokio::test]
async fn read_from_u64() {
    let mut mock = Builder::new().read_number(42).build();
    let value = mock.read_value::<TestFromU64>().await.unwrap();
    assert_eq!(TestFromU64, value);
}

#[tokio::test]
async fn read_from_u64_reader_error() {
    let mut mock = Builder::new()
        .read_number_error(Error::InvalidData("Bad reader".into()))
        .build();
    let err = mock.read_value::<TestFromU64>().await.unwrap_err();
    assert_eq!(Error::InvalidData("Bad reader".into()), err);
}

#[derive(Debug, PartialEq, Eq, NixDeserialize)]
enum TestEnum {
    #[nix(version = "..=19")]
    Pre20(TestTryFromU64),
    #[nix(version = "20..")]
    Post20(StructVersionTest),
}

#[tokio::test]
async fn read_enum_19() {
    let mut mock = Builder::new().version((1, 19)).read_number(42).build();
    let value = mock.read_value::<TestEnum>().await.unwrap();
    assert_eq!(TestEnum::Pre20(TestTryFromU64), value);
}

#[tokio::test]
async fn read_enum_20() {
    let mut mock = Builder::new()
        .version((1, 20))
        .read_number(42)
        .read_slice(b"klomp")
        .build();
    let value = mock.read_value::<TestEnum>().await.unwrap();
    assert_eq!(
        TestEnum::Post20(StructVersionTest {
            test: 42,
            hello: "klomp".into(),
        }),
        value
    );
}

#[tokio::test]
async fn read_enum_reader_error() {
    let mut mock = Builder::new()
        .version((1, 19))
        .read_number_error(Error::InvalidData("Bad reader".into()))
        .build();
    let err = mock.read_value::<TestEnum>().await.unwrap_err();
    assert_eq!(Error::InvalidData("Bad reader".into()), err);
}

#[tokio::test]
async fn read_enum_invalid_data_19() {
    let mut mock = Builder::new().version((1, 19)).read_number(666).build();
    let err = mock.read_value::<TestEnum>().await.unwrap_err();
    assert_eq!(Error::InvalidData("666".into()), err);
}

#[tokio::test]
async fn read_enum_invalid_data_20() {
    let mut mock = Builder::new()
        .version((1, 20))
        .read_number(666)
        .read_slice(b"The quick brown \xED\xA0\x80 jumped.")
        .build();
    let err = mock.read_value::<TestEnum>().await.unwrap_err();
    assert_eq!(
        Error::InvalidData("invalid utf-8 sequence of 1 bytes from index 16".into()),
        err
    );
}
