use serde::{Deserialize, Serialize};

// This function is required by serde to deserialize files
// with missing keys.
fn default_resource() -> String {
    "".to_string()
}

#[derive(Serialize, Deserialize)]
pub struct Output {
    pub path: String,
    #[serde(default = "default_resource")]
    pub hash_algorithm: String,
    #[serde(default = "default_resource")]
    pub hash: String,
}
