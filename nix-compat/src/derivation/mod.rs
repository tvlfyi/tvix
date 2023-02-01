use crate::store_path::{self, StorePath};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

mod errors;
mod output;
mod string_escape;
mod utils;
mod validate;
mod write;

#[cfg(test)]
mod tests;

// Public API of the crate.
pub use errors::{DerivationError, OutputError};
pub use output::{Hash, Output};
pub use utils::path_with_references;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Derivation {
    #[serde(rename = "args")]
    pub arguments: Vec<String>,

    pub builder: String,

    #[serde(rename = "env")]
    pub environment: BTreeMap<String, String>,

    #[serde(rename = "inputDrvs")]
    pub input_derivations: BTreeMap<String, BTreeSet<String>>,

    #[serde(rename = "inputSrcs")]
    pub input_sources: BTreeSet<String>,

    pub outputs: BTreeMap<String, Output>,

    pub system: String,
}

impl Derivation {
    /// write the Derivation to the given [std::fmt::Write], in ATerm format.
    ///
    /// The only errors returns are these when writing to the passed writer.
    pub fn serialize(&self, writer: &mut impl std::fmt::Write) -> Result<(), std::fmt::Error> {
        writer.write_str(write::DERIVATION_PREFIX)?;
        writer.write_char(write::PAREN_OPEN)?;

        write::write_outputs(writer, &self.outputs)?;
        write::write_input_derivations(writer, &self.input_derivations)?;
        write::write_input_sources(writer, &self.input_sources)?;
        write::write_system(writer, &self.system)?;
        write::write_builder(writer, &self.builder)?;
        write::write_arguments(writer, &self.arguments)?;
        write::write_enviroment(writer, &self.environment)?;

        writer.write_char(write::PAREN_CLOSE)?;

        Ok(())
    }

    /// return the ATerm serialization as a string.
    pub fn to_aterm_string(&self) -> String {
        let mut buffer = String::new();

        // invoke serialize and write to the buffer.
        // Note we only propagate errors writing to the writer in serialize,
        // which won't panic for the string we write to.
        self.serialize(&mut buffer).unwrap();

        buffer
    }

    /// Returns the fixed output path and its hash
    // (if the Derivation is fixed output),
    /// or None if there is no fixed output.
    /// This takes some shortcuts in case more than one output exists, as this
    /// can't be a valid fixed-output Derivation.
    pub fn get_fixed_output(&self) -> Option<(&String, &Hash)> {
        if self.outputs.len() != 1 {
            return None;
        }

        match self.outputs.get("out") {
            #[allow(clippy::manual_map)]
            Some(out_output) => match &out_output.hash {
                Some(out_output_hash) => Some((&out_output.path, out_output_hash)),
                // There has to be a hash, otherwise it would not be FOD
                None => None,
            },
            None => None,
        }
    }

    /// Returns the drv path of a Derivation struct.
    ///
    /// The drv path is calculated like this:
    ///   - Write the fingerprint of the Derivation to the sha256 hash function.
    ///     This is: `text:`,
    ///     all d.InputDerivations and d.InputSources (sorted, separated by a `:`),
    ///     a `:`,
    ///     a `sha256:`, followed by the sha256 digest of the ATerm representation (hex-encoded)
    ///     a `:`,
    ///     the storeDir, followed by a `:`,
    ///     the name of a derivation,
    ///     a `.drv`.
    ///   - Write the .drv A-Term contents to a hash function
    ///   - Take the digest, run hash.CompressHash(digest, 20) on it.
    ///   - Encode it with nixbase32
    ///   - Use it (and the name) to construct a [StorePath].
    pub fn calculate_derivation_path(&self, name: &str) -> Result<StorePath, DerivationError> {
        let mut s = String::from("text:");

        // collect the list of paths from input_sources and input_derivations
        // into a (sorted, guaranteed by BTreeSet) list, and join them by :
        let concat_inputs: BTreeSet<String> = {
            let mut inputs = self.input_sources.clone();
            let input_derivation_keys: Vec<String> =
                self.input_derivations.keys().cloned().collect();
            inputs.extend(input_derivation_keys);
            inputs
        };

        for input in concat_inputs {
            s.push_str(&input);
            s.push(':');
        }

        // calculate the sha256 hash of the ATerm representation, and represent
        // it as a hex-encoded string (prefixed with sha256:).
        let aterm_digest = {
            let mut derivation_hasher = Sha256::new();
            derivation_hasher.update(self.to_aterm_string());
            derivation_hasher.finalize()
        };

        s.push_str(&format!(
            "sha256:{:x}:{}:{}.drv",
            aterm_digest,
            store_path::STORE_DIR,
            name,
        ));

        utils::build_store_path(true, &s, name)
    }

    /// Calculate the drv replacement string for a given derivation.
    ///
    /// This is either called on a struct without output paths populated,
    /// to provide the `drv_replacement_str` value for the `calculate_output_paths`
    /// function call, or called on a struct with output paths populated, to
    /// calculate / cache lookups for calls to fn_get_drv_replacement.
    ///
    /// `fn_get_drv_replacement` is used to look up the drv replacement strings
    /// for input_derivations the Derivation refers to.
    pub fn calculate_drv_replacement_str<F>(&self, fn_get_drv_replacement: F) -> String
    where
        F: Fn(&str) -> String,
    {
        let mut hasher = Sha256::new();
        let digest = match self.get_fixed_output() {
            Some((fixed_output_path, fixed_output_hash)) => {
                hasher.update(format!(
                    "fixed:out:{}:{}:{}",
                    &fixed_output_hash.algo, &fixed_output_hash.digest, fixed_output_path,
                ));
                hasher.finalize()
            }
            None => {
                let mut replaced_input_derivations: BTreeMap<String, BTreeSet<String>> =
                    BTreeMap::new();

                // For each input_derivation, look up the replacement.
                for (drv_path, input_derivation) in &self.input_derivations {
                    replaced_input_derivations.insert(
                        fn_get_drv_replacement(drv_path).to_string(),
                        input_derivation.clone(),
                    );
                }

                // construct a new derivation struct with these replaced input derivation strings
                let replaced_derivation = Derivation {
                    input_derivations: replaced_input_derivations,
                    ..self.clone()
                };

                // write the ATerm of that to the hash function
                hasher.update(replaced_derivation.to_aterm_string());

                hasher.finalize()
            }
        };

        format!("{:x}", digest)
    }

    /// This calculates all output paths of a Derivation and updates the struct.
    /// It requires the struct to be initially without output paths.
    /// This means, self.outputs[$outputName].path needs to be an empty string,
    /// and self.environment[$outputName] needs to be an empty string.
    ///
    /// Output path calculation requires knowledge of "drv replacement
    /// strings", and in case of non-fixed-output derivations, also knowledge
    /// of "drv replacement" strings (recursively) of all input derivations.
    ///
    /// We solve this by asking the caller of this function to provide
    /// the drv replacement string of the current derivation itself,
    /// which is ran on the struct without output paths.
    ///
    /// This sound terribly ugly, but won't be too much of a concern later on, as
    /// naming fixed-output paths once uploaded will be a tvix-store concern,
    /// so there's no need to calculate them here anymore.
    ///
    /// On completion, self.environment[$outputName] and
    /// self.outputs[$outputName].path are set to the calculated output path for all
    /// outputs.
    pub fn calculate_output_paths(
        &mut self,
        name: &str,
        drv_replacement_str: &str,
    ) -> Result<(), DerivationError> {
        // Check if the Derivation is fixed output, because they cause
        // different fingerprints to be hashed.
        match self.get_fixed_output() {
            None => {
                // The fingerprint and hash differs per output
                for (output_name, output) in self.outputs.iter_mut() {
                    // Assert that outputs are not yet populated, to avoid using this function wrongly.
                    // We don't also go over self.environment, but it's a sufficient
                    // footgun prevention mechanism.
                    assert!(output.path.is_empty());

                    // calculate the output_name_path, which is the part of the NixPath after the digest.
                    let mut output_path_name = name.to_string();
                    if output_name != "out" {
                        output_path_name.push('-');
                        output_path_name.push_str(output_name);
                    }

                    let s = &format!(
                        "output:{}:sha256:{}:{}:{}",
                        output_name,
                        drv_replacement_str,
                        store_path::STORE_DIR,
                        output_path_name,
                    );

                    let abs_store_path =
                        utils::build_store_path(false, s, &output_path_name)?.to_absolute_path();

                    output.path = abs_store_path.clone();
                    self.environment
                        .insert(output_name.to_string(), abs_store_path);
                }
            }
            Some((fixed_output_path, fixed_output_hash)) => {
                // Assert that outputs are not yet populated, to avoid using this function wrongly.
                // We don't also go over self.environment, but it's a sufficient
                // footgun prevention mechanism.
                assert!(fixed_output_path.is_empty());

                let s = {
                    let mut s = String::new();
                    // Fixed-output derivation.
                    // There's two different hashing strategies in place, depending on the value of hash.algo.
                    // This code is _weird_ but it is what Nix is doing. See:
                    // https://github.com/NixOS/nix/blob/1385b2007804c8a0370f2a6555045a00e34b07c7/src/libstore/store-api.cc#L178-L196
                    if fixed_output_hash.algo == "r:sha256" {
                        s.push_str(&format!(
                            "source:sha256:{}",
                            fixed_output_hash.digest, // nixbase32
                        ));
                    } else {
                        s.push_str("output:out:sha256:");
                        // This is drv_replacement for FOD, with an empty fixed_output_path.
                        s.push_str(drv_replacement_str);
                    }
                    s.push_str(&format!(":{}:{}", store_path::STORE_DIR, name));
                    s
                };

                let abs_store_path = utils::build_store_path(false, &s, name)?.to_absolute_path();

                self.outputs.insert(
                    "out".to_string(),
                    Output {
                        path: abs_store_path.clone(),
                        hash: Some(fixed_output_hash.clone()),
                    },
                );
                self.environment.insert("out".to_string(), abs_store_path);
            }
        };

        Ok(())
    }
}
