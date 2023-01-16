use serde::{Deserialize, Serialize};
use tvix_store::store_path::StorePath;

use crate::OutputError;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub path: String,

    #[serde(flatten)]
    pub hash: Option<Hash>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hash {
    #[serde(rename = "hash")]
    pub digest: String,
    #[serde(rename = "hashAlgo")]
    pub algo: String,
}

impl Output {
    pub fn is_fixed(&self) -> bool {
        self.hash.is_some()
    }

    pub fn validate(&self, validate_output_paths: bool) -> Result<(), OutputError> {
        // TODO: add validation for hash, hashAlgo
        if validate_output_paths {
            if let Err(e) = StorePath::from_absolute_path(&self.path) {
                return Err(OutputError::InvalidOutputPath(self.path.to_string(), e));
            }
        }
        Ok(())
    }
}
