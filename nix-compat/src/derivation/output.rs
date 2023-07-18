use crate::derivation::OutputError;
use crate::nixhash::{HashAlgo, NixHashWithMode};
use crate::store_path::StorePath;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub path: String,

    #[serde(flatten)]
    pub hash_with_mode: Option<NixHashWithMode>,
}

impl Output {
    pub fn is_fixed(&self) -> bool {
        self.hash_with_mode.is_some()
    }

    pub fn validate(&self, validate_output_paths: bool) -> Result<(), OutputError> {
        if let Some(hash) = &self.hash_with_mode {
            match hash {
                NixHashWithMode::Flat(h) | NixHashWithMode::Recursive(h) => {
                    if h.algo != HashAlgo::Sha1 || h.algo != HashAlgo::Sha256 {
                        return Err(OutputError::InvalidHashAlgo(h.algo.to_string()));
                    }
                }
            }
        }
        if validate_output_paths {
            if let Err(e) = StorePath::from_absolute_path(self.path.as_bytes()) {
                return Err(OutputError::InvalidOutputPath(self.path.to_string(), e));
            }
        }
        Ok(())
    }
}
