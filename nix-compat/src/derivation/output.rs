use crate::derivation::OutputError;
use crate::{nixbase32, store_path::StorePath};
use serde::{Deserialize, Serialize};

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
        if let Some(hash) = &self.hash {
            // try to decode digest
            let result = nixbase32::decode(&hash.digest.as_bytes());
            match result {
                Err(e) => return Err(OutputError::InvalidHashEncoding(hash.digest.clone(), e)),
                Ok(digest) => {
                    if hash.algo != "sha1" && hash.algo != "sha256" {
                        return Err(OutputError::InvalidHashAlgo(hash.algo.to_string()));
                    }
                    if (hash.algo == "sha1" && digest.len() != 20)
                        || (hash.algo == "sha256" && digest.len() != 32)
                    {
                        return Err(OutputError::InvalidDigestSizeForAlgo(
                            digest.len(),
                            hash.algo.to_string(),
                        ));
                    }
                }
            };
        }
        if validate_output_paths {
            if let Err(e) = StorePath::from_absolute_path(&self.path) {
                return Err(OutputError::InvalidOutputPath(self.path.to_string(), e));
            }
        }
        Ok(())
    }
}
