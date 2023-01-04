use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Output {
    pub path: String,

    #[serde(flatten)]
    pub hash: Option<Hash>,
}

#[derive(Serialize, Deserialize)]
pub struct Hash {
    #[serde(rename = "hash")]
    pub digest: String,
    #[serde(rename = "hash_algorithm")]
    pub algo: String,
}

impl Output {
    pub fn is_fixed(&self) -> bool {
        self.hash.is_some()
    }
}
