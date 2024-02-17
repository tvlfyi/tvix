use crate::derivation::OutputError;
use crate::nixhash::CAHash;
use crate::store_path::StorePathRef;
use serde::{Deserialize, Serialize};
use serde_json::Map;

/// References the derivation output.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Output {
    /// Store path of build result.
    pub path: String,

    #[serde(flatten)]
    pub ca_hash: Option<CAHash>, // we can only represent a subset here.
}

impl<'de> Deserialize<'de> for Output {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let fields = Map::deserialize(deserializer)?;
        Ok(Self {
            path: fields
                .get("path")
                .ok_or(serde::de::Error::missing_field(
                    "`path` is missing but required for outputs",
                ))?
                .as_str()
                .ok_or(serde::de::Error::invalid_type(
                    serde::de::Unexpected::Other("certainly not a string"),
                    &"a string",
                ))?
                .to_owned(),
            ca_hash: CAHash::from_map::<D>(&fields)?,
        })
    }
}

impl Output {
    pub fn is_fixed(&self) -> bool {
        self.ca_hash.is_some()
    }

    pub fn validate(&self, validate_output_paths: bool) -> Result<(), OutputError> {
        if let Some(fixed_output_hash) = &self.ca_hash {
            match fixed_output_hash {
                CAHash::Flat(_) | CAHash::Nar(_) => {
                    // all hashes allowed for Flat, and Nar.
                }
                _ => return Err(OutputError::InvalidCAHash(fixed_output_hash.clone())),
            }
        }

        if validate_output_paths {
            if let Err(e) = StorePathRef::from_absolute_path(self.path.as_bytes()) {
                return Err(OutputError::InvalidOutputPath(self.path.to_string(), e));
            }
        }
        Ok(())
    }
}

/// This ensures that a potentially valid input addressed
/// output is deserialized as a non-fixed output.
#[test]
fn deserialize_valid_input_addressed_output() {
    let json_bytes = r#"
    {
      "path": "/nix/store/blablabla"
    }"#;
    let output: Output = serde_json::from_str(json_bytes).expect("must parse");

    assert!(!output.is_fixed());
}

/// This ensures that a potentially valid fixed output
/// output deserializes fine as a fixed output.
#[test]
fn deserialize_valid_fixed_output() {
    let json_bytes = r#"
    {
        "path": "/nix/store/blablablabla",
        "hash": "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba",
        "hashAlgo": "r:sha256"
    }"#;
    let output: Output = serde_json::from_str(json_bytes).expect("must parse");

    assert!(output.is_fixed());
}

/// This ensures that parsing an input with the invalid hash encoding
/// will result in a parsing failure.
#[test]
fn deserialize_with_error_invalid_hash_encoding_fixed_output() {
    let json_bytes = r#"
    {
        "path": "/nix/store/blablablabla",
        "hash": "IAMNOTVALIDNIXBASE32",
        "hashAlgo": "r:sha256"
    }"#;
    let output: Result<Output, _> = serde_json::from_str(json_bytes);

    assert!(output.is_err());
}

/// This ensures that parsing an input with the wrong hash algo
/// will result in a parsing failure.
#[test]
fn deserialize_with_error_invalid_hash_algo_fixed_output() {
    let json_bytes = r#"
    {
        "path": "/nix/store/blablablabla",
        "hash": "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba",
        "hashAlgo": "r:sha1024"
    }"#;
    let output: Result<Output, _> = serde_json::from_str(json_bytes);

    assert!(output.is_err());
}

/// This ensures that parsing an input with the missing hash algo but present hash will result in a
/// parsing failure.
#[test]
fn deserialize_with_error_missing_hash_algo_fixed_output() {
    let json_bytes = r#"
    {
        "path": "/nix/store/blablablabla",
        "hash": "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba",
    }"#;
    let output: Result<Output, _> = serde_json::from_str(json_bytes);

    assert!(output.is_err());
}

/// This ensures that parsing an input with the missing hash but present hash algo will result in a
/// parsing failure.
#[test]
fn deserialize_with_error_missing_hash_fixed_output() {
    let json_bytes = r#"
    {
        "path": "/nix/store/blablablabla",
        "hashAlgo": "r:sha1024"
    }"#;
    let output: Result<Output, _> = serde_json::from_str(json_bytes);

    assert!(output.is_err());
}
