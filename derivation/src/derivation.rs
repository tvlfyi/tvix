use crate::nix_hash;
use crate::output::{Hash, Output};
use crate::write;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::{collections::BTreeMap, fmt, fmt::Write};
use tvix_store::nixbase32::NIXBASE32;
use tvix_store::store_path::{ParseStorePathError, StorePath, STORE_DIR};

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
    pub input_sources: Vec<String>,

    pub outputs: BTreeMap<String, Output>,

    pub system: String,
}

/// This returns a store path, either of a derivation or a regular output.
/// The path_hash is compressed to 20 bytes, and nixbase32-encoded (32 characters)
fn build_store_path(
    is_derivation: bool,
    path_hash: &[u8],
    name: &str,
) -> Result<StorePath, ParseStorePathError> {
    let compressed = nix_hash::compress_hash(path_hash, 20);
    if is_derivation {
        StorePath::from_string(
            format!(
                "{}-{}{}",
                NIXBASE32.encode(&compressed),
                name,
                write::DOT_FILE_EXT,
            )
            .as_str(),
        )
    } else {
        StorePath::from_string(format!("{}-{}", NIXBASE32.encode(&compressed), name,).as_str())
    }
}

impl Derivation {
    pub fn serialize(&self, writer: &mut impl Write) -> Result<(), fmt::Error> {
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
    pub fn calculate_derivation_path(&self, name: &str) -> Result<StorePath, ParseStorePathError> {
        let mut hasher = Sha256::new();

        // collect the list of paths from input_sources and input_derivations
        // into a sorted list, and join them by :
        hasher.update(write::TEXT_COLON);

        let concat_inputs: Vec<String> = {
            let mut inputs = self.input_sources.clone();
            let input_derivation_keys: Vec<String> =
                self.input_derivations.keys().cloned().collect();
            inputs.extend(input_derivation_keys);
            inputs.sort();
            inputs
        };

        for input in concat_inputs {
            hasher.update(input);
            hasher.update(write::COLON);
        }

        // calculate the sha256 hash of the ATerm representation, and represent
        // it as a hex-encoded string (prefixed with sha256:).
        hasher.update(write::SHA256_COLON);

        let digest = {
            let mut derivation_hasher = Sha256::new();
            derivation_hasher.update(self.to_string());
            derivation_hasher.finalize()
        };

        hasher.update(format!("{:x}", digest));
        hasher.update(write::COLON);
        hasher.update(STORE_DIR);
        hasher.update(write::COLON);

        hasher.update(name);
        hasher.update(write::DOT_FILE_EXT);

        build_store_path(true, &hasher.finalize(), name)
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
                hasher.update("fixed:out:");
                hasher.update(&fixed_output_hash.algo);
                hasher.update(":");
                hasher.update(&fixed_output_hash.digest);
                hasher.update(":");
                hasher.update(fixed_output_path);
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
                hasher.update(replaced_derivation.to_string());

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
    ) -> Result<(), ParseStorePathError> {
        let mut hasher = Sha256::new();

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

                    hasher.update("output:");
                    hasher.update(output_name);
                    hasher.update(":sha256:");
                    hasher.update(drv_replacement_str);
                    hasher.update(":");
                    hasher.update(STORE_DIR);
                    hasher.update(":");

                    // calculate the output_name_path, which is the part of the NixPath after the digest.
                    let mut output_path_name = name.to_string();
                    if output_name != "out" {
                        output_path_name.push('-');
                        output_path_name.push_str(output_name);
                    }

                    hasher.update(output_path_name.as_str());

                    let digest = hasher.finalize_reset();

                    let abs_store_path =
                        build_store_path(false, &digest, &output_path_name)?.to_absolute_path();

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

                let digest = {
                    // Fixed-output derivation.
                    // There's two different hashing strategies in place, depending on the value of hash.algo.
                    // This code is _weird_ but it is what Nix is doing. See:
                    // https://github.com/NixOS/nix/blob/1385b2007804c8a0370f2a6555045a00e34b07c7/src/libstore/store-api.cc#L178-L196
                    if fixed_output_hash.algo == "r:sha256" {
                        hasher.update("source:");
                        hasher.update("sha256");
                        hasher.update(":");
                        hasher.update(fixed_output_hash.digest.clone()); // nixbase32
                    } else {
                        hasher.update("output:out:sha256:");
                        // This is drv_replacement for FOD, with an empty fixed_output_path.
                        hasher.update(drv_replacement_str);
                    }
                    hasher.update(":");
                    hasher.update(STORE_DIR);
                    hasher.update(":");
                    hasher.update(name);
                    hasher.finalize()
                };

                let abs_store_path = build_store_path(false, &digest, name)?.to_absolute_path();

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

impl fmt::Display for Derivation {
    /// Formats the Derivation in ATerm representation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.serialize(f)
    }
}
