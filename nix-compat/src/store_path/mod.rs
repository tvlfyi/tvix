use crate::nixbase32;
use data_encoding::{DecodeError, BASE64};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::PathBuf,
    str::{self, FromStr},
};
use thiserror;

#[cfg(target_family = "unix")]
use std::os::unix::ffi::OsStringExt;

mod utils;

pub use utils::*;

pub const DIGEST_SIZE: usize = 20;
pub const ENCODED_DIGEST_SIZE: usize = nixbase32::encode_len(DIGEST_SIZE);

// The store dir prefix, without trailing slash.
// That's usually where the Nix store is mounted at.
pub const STORE_DIR: &str = "/nix/store";
pub const STORE_DIR_WITH_SLASH: &str = "/nix/store/";

/// Errors that can occur when parsing a literal store path
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Dash is missing between hash and name")]
    MissingDash,
    #[error("Hash encoding is invalid: {0}")]
    InvalidHashEncoding(DecodeError),
    #[error("Invalid length")]
    InvalidLength,
    #[error(
        "Invalid name: \"{}\", character at position {} is invalid",
        std::str::from_utf8(.0).unwrap_or(&BASE64.encode(.0)),
        .1,
    )]
    InvalidName(Vec<u8>, u8),
    #[error("Tried to parse an absolute path which was missing the store dir prefix.")]
    MissingStoreDir,
}

/// Represents a path in the Nix store (a direct child of [STORE_DIR]).
///
/// It consists of a digest (20 bytes), and a name, which is a string.
/// The name may only contain ASCII alphanumeric, or one of the following
/// characters: `-`, `_`, `.`, `+`, `?`, `=`.
/// The name is usually used to describe the pname and version of a package.
/// Derivation paths can also be represented as store paths, their names just
/// end with the `.drv` prefix.
///
/// A [StorePath] does not encode any additional subpath "inside" the store
/// path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StorePath {
    digest: [u8; DIGEST_SIZE],
    name: String,
}

impl StorePath {
    pub fn digest(&self) -> &[u8; DIGEST_SIZE] {
        &self.digest
    }

    pub fn name(&self) -> &str {
        self.name.as_ref()
    }
}

impl PartialOrd for StorePath {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// `StorePath`s are sorted by their reverse digest to match the sorting order
/// of the nixbase32-encoded string.
impl Ord for StorePath {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let order = self.digest.iter().rev().cmp(other.digest.iter().rev());

        // This order must match the order of the nixbase32 encoded digests.
        #[cfg(debug_assertions)]
        {
            let self_hash = nixbase32::encode(&self.digest);
            let other_hash = nixbase32::encode(&other.digest);
            let canonical_order = self_hash.cmp(&other_hash);
            assert_eq!(
                order, canonical_order,
                "Ordering of nixbase32 differs, {:?} instead of {:?}:\n{:?}\n{:?}",
                order, canonical_order, self_hash, other_hash
            );
        }

        order
    }
}

impl FromStr for StorePath {
    type Err = Error;

    /// Construct a [StorePath] by passing the `$digest-$name` string
    /// that comes after [STORE_DIR_WITH_SLASH].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_bytes(s.as_bytes())
    }
}

impl StorePath {
    /// Construct a [StorePath] by passing the `$digest-$name` string
    /// that comes after [STORE_DIR_WITH_SLASH].
    pub fn from_bytes(s: &[u8]) -> Result<StorePath, Error> {
        Ok(StorePathRef::from_bytes(s)?.to_owned())
    }

    /// Decompose a string into a [StorePath] and a [PathBuf] containing the
    /// rest of the path, or an error.
    #[cfg(target_family = "unix")]
    pub fn from_absolute_path_full(s: &str) -> Result<(StorePath, PathBuf), Error> {
        // strip [STORE_DIR_WITH_SLASH] from s
        match s.strip_prefix(STORE_DIR_WITH_SLASH) {
            None => Err(Error::MissingStoreDir),
            Some(rest) => {
                // put rest in a PathBuf
                let mut p = PathBuf::new();
                p.push(rest);

                let mut it = p.components();

                // The first component of the rest must be parse-able as a [StorePath]
                if let Some(first_component) = it.next() {
                    // convert first component to StorePath
                    let first_component_bytes = first_component.as_os_str().to_owned().into_vec();
                    let store_path = StorePath::from_bytes(&first_component_bytes)?;
                    // collect rest
                    let rest_buf: PathBuf = it.collect();
                    Ok((store_path, rest_buf))
                } else {
                    Err(Error::InvalidLength) // Well, or missing "/"?
                }
            }
        }
    }

    /// Returns an absolute store path string.
    /// That is just the string representation, prefixed with the store prefix
    /// ([STORE_DIR_WITH_SLASH]),
    pub fn to_absolute_path(&self) -> String {
        let sp_ref: StorePathRef = self.into();
        sp_ref.to_absolute_path()
    }
}

impl<'de> Deserialize<'de> for StorePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let r = <StorePathRef<'de> as Deserialize<'de>>::deserialize(deserializer)?;
        Ok(r.to_owned())
    }
}

impl Serialize for StorePath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let r: StorePathRef = self.into();
        r.serialize(serializer)
    }
}

/// Like [StorePath], but without a heap allocation for the name.
/// Used by [StorePath] for parsing.
///
#[derive(Debug, Eq, PartialEq)]
pub struct StorePathRef<'a> {
    digest: [u8; DIGEST_SIZE],
    name: &'a str,
}

impl<'a> From<&'a StorePath> for StorePathRef<'a> {
    fn from(&StorePath { digest, ref name }: &'a StorePath) -> Self {
        StorePathRef {
            digest,
            name: name.as_ref(),
        }
    }
}

impl<'a> StorePathRef<'a> {
    pub fn digest(&self) -> &[u8; DIGEST_SIZE] {
        &self.digest
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn to_owned(&self) -> StorePath {
        StorePath {
            digest: self.digest,
            name: self.name.to_owned(),
        }
    }

    /// Construct a [StorePathRef] from a name and digest.
    pub fn from_name_and_digest(name: &'a str, digest: &[u8]) -> Result<Self, Error> {
        Ok(Self {
            name: validate_name(name.as_bytes())?,
            digest: digest.try_into().map_err(|_| Error::InvalidLength)?,
        })
    }

    /// Construct a [StorePathRef] from an absolute store path string.
    /// This is equivalent to calling [StorePathRef::from_bytes], but stripping
    /// the [STORE_DIR_WITH_SLASH] prefix before.
    pub fn from_absolute_path(s: &'a [u8]) -> Result<Self, Error> {
        match s.strip_prefix(STORE_DIR_WITH_SLASH.as_bytes()) {
            Some(s_stripped) => Self::from_bytes(s_stripped),
            None => Err(Error::MissingStoreDir),
        }
    }

    /// Construct a [StorePathRef] by passing the `$digest-$name` string
    /// that comes after [STORE_DIR_WITH_SLASH].
    pub fn from_bytes(s: &'a [u8]) -> Result<Self, Error> {
        // the whole string needs to be at least:
        //
        // - 32 characters (encoded hash)
        // - 1 dash
        // - 1 character for the name
        if s.len() < ENCODED_DIGEST_SIZE + 2 {
            Err(Error::InvalidLength)?
        }

        let digest = nixbase32::decode_fixed(&s[..ENCODED_DIGEST_SIZE])
            .map_err(Error::InvalidHashEncoding)?;

        if s[ENCODED_DIGEST_SIZE] != b'-' {
            return Err(Error::MissingDash);
        }

        Ok(StorePathRef {
            digest,
            name: validate_name(&s[ENCODED_DIGEST_SIZE + 1..])?,
        })
    }

    /// Returns an absolute store path string.
    /// That is just the string representation, prefixed with the store prefix
    /// ([STORE_DIR_WITH_SLASH]),
    pub fn to_absolute_path(&self) -> String {
        format!("{}{}", STORE_DIR_WITH_SLASH, self)
    }
}

impl<'de> Deserialize<'de> for StorePathRef<'de> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let string: &'de str = Deserialize::deserialize(deserializer)?;
        let stripped: Option<&str> = string.strip_prefix(STORE_DIR_WITH_SLASH);
        let stripped: &str = stripped.ok_or_else(|| {
            serde::de::Error::invalid_value(
                serde::de::Unexpected::Str(string),
                &"store path prefix",
            )
        })?;
        StorePathRef::from_bytes(stripped.as_bytes()).map_err(|_| {
            serde::de::Error::invalid_value(serde::de::Unexpected::Str(string), &"StorePath")
        })
    }
}

impl Serialize for StorePathRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let string: String = self.to_absolute_path();
        string.serialize(serializer)
    }
}

/// NAME_CHARS contains `true` for bytes that are valid in store path names,
/// not accounting for '.' being permitted only past the first character.
static NAME_CHARS: [bool; 256] = {
    let mut tbl = [false; 256];
    let mut c = 0;

    loop {
        tbl[c as usize] = matches!(c, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'+' | b'-' | b'_' | b'?' | b'=' | b'.');

        if c == u8::MAX {
            break;
        }

        c += 1;
    }

    tbl
};

/// Checks a given &[u8] to match the restrictions for [StorePath::name], and
/// returns the name as string if successful.
pub(crate) fn validate_name(s: &(impl AsRef<[u8]> + ?Sized)) -> Result<&str, Error> {
    let s = s.as_ref();

    // Empty or excessively long names are not allowed.
    if s.is_empty() || s.len() > 211 {
        return Err(Error::InvalidLength);
    }

    if s[0] == b'.' {
        return Err(Error::InvalidName(s.to_vec(), 0));
    }

    let mut valid = true;
    for &c in s {
        valid = valid && NAME_CHARS[c as usize];
    }

    if !valid {
        for (i, &c) in s.iter().enumerate() {
            if !NAME_CHARS[c as usize] {
                return Err(Error::InvalidName(s.to_vec(), i as u8));
            }
        }

        unreachable!();
    }

    // SAFETY: We permit a subset of ASCII, which guarantees valid UTF-8.
    Ok(unsafe { str::from_utf8_unchecked(s) })
}

impl fmt::Display for StorePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        StorePathRef::from(self).fmt(f)
    }
}

impl fmt::Display for StorePathRef<'_> {
    /// The string representation of a store path starts with a digest (20
    /// bytes), [crate::nixbase32]-encoded, followed by a `-`,
    /// and ends with the name.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", nixbase32::encode(&self.digest), self.name)
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;
    use std::path::PathBuf;

    use crate::store_path::{StorePathRef, DIGEST_SIZE};
    use hex_literal::hex;
    use pretty_assertions::assert_eq;
    use test_case::test_case;

    use super::{Error, StorePath};

    #[test]
    fn happy_path() {
        let example_nix_path_str =
            "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432";
        let nixpath = StorePath::from_bytes(example_nix_path_str.as_bytes())
            .expect("Error parsing example string");

        let expected_digest: [u8; DIGEST_SIZE] = hex!("8a12321522fd91efbd60ebb2481af88580f61600");

        assert_eq!("net-tools-1.60_p20170221182432", nixpath.name);
        assert_eq!(nixpath.digest, expected_digest);

        assert_eq!(example_nix_path_str, nixpath.to_string())
    }

    #[test]
    fn store_path_ordering() {
        let store_paths = [
            "/nix/store/0lk5dgi01r933abzfj9c9wlndg82yd3g-psutil-5.9.6.tar.gz.drv",
            "/nix/store/1xj43bva89f9qmwm37zl7r3d7m67i9ck-shorttoc-1.3-tex.drv",
            "/nix/store/2gb633czchi20jq1kqv70rx2yvvgins8-lifted-base-0.2.3.12.tar.gz.drv",
            "/nix/store/2vksym3r3zqhp15q3fpvw2mnvffv11b9-docbook-xml-4.5.zip.drv",
            "/nix/store/5q918awszjcz5720xvpc2czbg1sdqsf0-rust_renaming-0.1.0-lib",
            "/nix/store/7jw30i342sr2p1fmz5xcfnch65h4zbd9-dbus-1.14.10.tar.xz.drv",
            "/nix/store/96yqwqhnp3qya4rf4n0rcl0lwvrylp6k-eap8021x-222.40.1.tar.gz.drv",
            "/nix/store/9gjqg36a1v0axyprbya1hkaylmnffixg-virtualenv-20.24.5.tar.gz.drv",
            "/nix/store/a4i5mci2g9ada6ff7ks38g11dg6iqyb8-perl-5.32.1.drv",
            "/nix/store/a5g76ljava4h5pxlggz3aqdhs3a4fk6p-ToolchainInfo.plist.drv",
            "/nix/store/db46l7d6nswgz4ffp1mmd56vjf9g51v6-version.plist.drv",
            "/nix/store/g6f7w20sd7vwy0rc1r4bfsw4ciclrm4q-crates-io-num_cpus-1.12.0.drv",
            "/nix/store/iw82n1wwssb8g6772yddn8c3vafgv9np-bootstrap-stage1-sysctl-stdenv-darwin.drv",
            "/nix/store/lp78d1y5wxpcn32d5c4r7xgbjwiw0cgf-logo.svg.drv",
            "/nix/store/mf00ank13scv1f9l1zypqdpaawjhfr3s-python3.11-psutil-5.9.6.drv",
            "/nix/store/mpfml61ra7pz90124jx9r3av0kvkz2w1-perl5.36.0-Encode-Locale-1.05",
            "/nix/store/qhsvwx4h87skk7c4mx0xljgiy3z93i23-source.drv",
            "/nix/store/riv7d73adim8hq7i04pr8kd0jnj93nav-fdk-aac-2.0.2.tar.gz.drv",
            "/nix/store/s64b9031wga7vmpvgk16xwxjr0z9ln65-human-signals-5.0.0.tgz-extracted",
            "/nix/store/w6svg3m2xdh6dhx0gl1nwa48g57d3hxh-thiserror-1.0.49",
        ];

        for w in store_paths.windows(2) {
            if w.len() < 2 {
                continue;
            }
            let (pa, _) = StorePath::from_absolute_path_full(w[0]).expect("parseable");
            let (pb, _) = StorePath::from_absolute_path_full(w[1]).expect("parseable");
            assert_eq!(
                Ordering::Less,
                pa.cmp(&pb),
                "{:?} not less than {:?}",
                w[0],
                w[1]
            );
        }
    }

    /// This is the store path rejected when `nix-store --add`'ing an
    /// empty `.gitignore` file.
    ///
    /// Nix 2.4 accidentally dropped this behaviour, but this is considered a bug.
    /// See https://github.com/NixOS/nix/pull/9095.
    #[test]
    fn starts_with_dot() {
        StorePath::from_bytes(b"fli4bwscgna7lpm7v5xgnjxrxh0yc7ra-.gitignore")
            .expect_err("must fail");
    }

    #[test]
    fn empty_name() {
        StorePath::from_bytes(b"00bgd045z0d4icpbc2yy-").expect_err("must fail");
    }

    #[test]
    fn excessive_length() {
        StorePath::from_bytes(b"00bgd045z0d4icpbc2yy-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .expect_err("must fail");
    }

    #[test]
    fn invalid_hash_length() {
        StorePath::from_bytes(b"00bgd045z0d4icpbc2yy-net-tools-1.60_p20170221182432")
            .expect_err("must fail");
    }

    #[test]
    fn invalid_encoding_hash() {
        StorePath::from_bytes(b"00bgd045z0d4icpbc2yyz4gx48aku4la-net-tools-1.60_p20170221182432")
            .expect_err("must fail");
    }

    #[test]
    fn more_than_just_the_bare_nix_store_path() {
        StorePath::from_bytes(
            b"00bgd045z0d4icpbc2yyz4gx48aku4la-net-tools-1.60_p20170221182432/bin/arp",
        )
        .expect_err("must fail");
    }

    #[test]
    fn no_dash_between_hash_and_name() {
        StorePath::from_bytes(b"00bgd045z0d4icpbc2yyz4gx48ak44lanet-tools-1.60_p20170221182432")
            .expect_err("must fail");
    }

    #[test]
    fn absolute_path() {
        let example_nix_path_str =
            "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432";
        let nixpath_expected =
            StorePathRef::from_bytes(example_nix_path_str.as_bytes()).expect("must parse");

        let nixpath_actual = StorePathRef::from_absolute_path(
            "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432".as_bytes(),
        )
        .expect("must parse");

        assert_eq!(nixpath_expected, nixpath_actual);

        assert_eq!(
            "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432",
            nixpath_actual.to_absolute_path(),
        );
    }

    #[test]
    fn absolute_path_missing_prefix() {
        assert_eq!(
            Error::MissingStoreDir,
            StorePathRef::from_absolute_path(b"foobar-123").expect_err("must fail")
        );
    }

    #[test]
    fn serialize_ref() {
        let nixpath_actual = StorePathRef::from_bytes(
            b"00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432",
        )
        .expect("can parse");

        let serialized = serde_json::to_string(&nixpath_actual).expect("can serialize");

        assert_eq!(
            "\"/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432\"",
            &serialized
        );
    }

    #[test]
    fn serialize_owned() {
        let nixpath_actual = StorePath::from_bytes(
            b"00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432",
        )
        .expect("can parse");

        let serialized = serde_json::to_string(&nixpath_actual).expect("can serialize");

        assert_eq!(
            "\"/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432\"",
            &serialized
        );
    }

    #[test]
    fn deserialize_ref() {
        let store_path_str_json =
            "\"/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432\"";

        let store_path: StorePath = serde_json::from_str(store_path_str_json).expect("valid json");

        assert_eq!(
            "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432",
            store_path.to_absolute_path()
        );
    }

    #[test_case(
        "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432",
        (StorePath::from_bytes(b"00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432").unwrap(), PathBuf::new())
    ; "without prefix")]
    #[test_case(
        "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432/",
        (StorePath::from_bytes(b"00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432").unwrap(), PathBuf::new())
    ; "without prefix, but trailing slash")]
    #[test_case(
        "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432/bin/arp",
        (StorePath::from_bytes(b"00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432").unwrap(), PathBuf::from("bin/arp"))
    ; "with prefix")]
    #[test_case(
        "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432/bin/arp/",
        (StorePath::from_bytes(b"00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432").unwrap(), PathBuf::from("bin/arp/"))
    ; "with prefix and trailing slash")]
    fn from_absolute_path_full(s: &str, expected: (StorePath, PathBuf)) {
        let actual = StorePath::from_absolute_path_full(s).expect("must succeed");
        assert_eq!(expected, actual);
    }

    #[test]
    fn from_absolute_path_errors() {
        assert_eq!(
            Error::InvalidLength,
            StorePath::from_absolute_path_full("/nix/store/").expect_err("must fail")
        );
        assert_eq!(
            Error::InvalidLength,
            StorePath::from_absolute_path_full("/nix/store/foo").expect_err("must fail")
        );
        assert_eq!(
            Error::MissingStoreDir,
            StorePath::from_absolute_path_full(
                "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432"
            )
            .expect_err("must fail")
        );
    }
}
