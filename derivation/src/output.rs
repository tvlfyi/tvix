use serde::{Deserialize, Serialize};
use tvix_store::nixpath::NixPath;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

    pub fn validate(&self) -> anyhow::Result<()> {
        NixPath::from_absolute_path(&self.path)?;
        Ok(())
    }
}
