use crate::store_path::{
    self, build_output_path, build_regular_ca_path, build_text_path, StorePath,
};
use bstr::BString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

mod errors;
mod escape;
mod output;
mod validate;
mod write;

#[cfg(test)]
mod tests;

// Public API of the crate.
pub use crate::nixhash::{NixHash, NixHashWithMode};
pub use errors::{DerivationError, OutputError};
pub use output::Output;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Derivation {
    #[serde(rename = "args")]
    pub arguments: Vec<String>,

    pub builder: String,

    #[serde(rename = "env")]
    pub environment: BTreeMap<String, BString>,

    #[serde(rename = "inputDrvs")]
    pub input_derivations: BTreeMap<String, BTreeSet<String>>,

    #[serde(rename = "inputSrcs")]
    pub input_sources: BTreeSet<String>,

    pub outputs: BTreeMap<String, Output>,

    pub system: String,
}

impl Derivation {
    /// write the Derivation to the given [std::io::Write], in ATerm format.
    ///
    /// The only errors returns are these when writing to the passed writer.
    pub fn serialize(&self, writer: &mut impl std::io::Write) -> Result<(), io::Error> {
        io::copy(&mut io::Cursor::new(write::DERIVATION_PREFIX), writer)?;
        write::write_char(writer, write::PAREN_OPEN)?;

        write::write_outputs(writer, &self.outputs)?;
        write::write_char(writer, write::COMMA)?;

        write::write_input_derivations(writer, &self.input_derivations)?;
        write::write_char(writer, write::COMMA)?;

        write::write_input_sources(writer, &self.input_sources)?;
        write::write_char(writer, write::COMMA)?;

        write::write_system(writer, &self.system)?;
        write::write_char(writer, write::COMMA)?;

        write::write_builder(writer, &self.builder)?;
        write::write_char(writer, write::COMMA)?;

        write::write_arguments(writer, &self.arguments)?;
        write::write_char(writer, write::COMMA)?;

        write::write_enviroment(writer, &self.environment)?;

        write::write_char(writer, write::PAREN_CLOSE)?;

        Ok(())
    }

    /// return the ATerm serialization.
    pub fn to_aterm_bytes(&self) -> Vec<u8> {
        let mut buffer: Vec<u8> = Vec::new();

        // invoke serialize and write to the buffer.
        // Note we only propagate errors writing to the writer in serialize,
        // which won't panic for the string we write to.
        self.serialize(&mut buffer).unwrap();

        buffer
    }

    /// Returns the drv path of a [Derivation] struct.
    ///
    /// The drv path is calculated by invoking [build_text_path], using
    /// the `name` with a `.drv` suffix as name, all [Derivation::input_sources] and
    /// keys of [Derivation::input_derivations] as references, and the ATerm string of
    /// the [Derivation] as content.
    pub fn calculate_derivation_path(&self, name: &str) -> Result<StorePath, DerivationError> {
        // append .drv to the name
        let name = &format!("{}.drv", name);

        // collect the list of paths from input_sources and input_derivations
        // into a (sorted, guaranteed by BTreeSet) list of references
        let references: BTreeSet<String> = {
            let mut inputs = self.input_sources.clone();
            let input_derivation_keys: Vec<String> =
                self.input_derivations.keys().cloned().collect();
            inputs.extend(input_derivation_keys);
            inputs
        };

        build_text_path(name, self.to_aterm_bytes(), references)
            .map_err(|_e| DerivationError::InvalidOutputName(name.to_string()))
    }

    /// Returns the FOD digest, if the derivation is fixed-output, or None if
    /// it's not.
    fn fod_digest(&self) -> Option<Vec<u8>> {
        if self.outputs.len() != 1 {
            return None;
        }

        let out_output = self.outputs.get("out")?;
        Some(
            Sha256::new_with_prefix(format!(
                "fixed:out:{}:{}",
                out_output.hash_with_mode.clone()?.to_nix_hash_string(),
                out_output.path
            ))
            .finalize()
            .to_vec(),
        )
    }

    /// Calculates the hash of a derivation modulo fixed-output subderivations.
    ///
    /// This is called `hashDerivationModulo` in nixcpp.
    ///
    /// It returns a [NixHash], created by calculating the sha256 digest of
    /// the derivation ATerm representation, except that:
    ///  -  any input derivation paths have beed replaced "by the result of a
    ///     recursive call to this function" and that
    ///  - for fixed-output derivations the special
    ///    `fixed:out:${algo}:${digest}:${fodPath}` string is hashed instead of
    ///    the A-Term.
    ///
    /// If the derivation is not a fixed derivation, it's up to the caller of
    /// this function to provide a lookup function to lookup these calculation
    /// results of parent derivations at `fn_get_hash_derivation_modulo` (by
    /// drv path).
    pub fn derivation_or_fod_hash<F>(&self, fn_get_derivation_or_fod_hash: F) -> NixHash
    where
        F: Fn(&str) -> NixHash,
    {
        // Fixed-output derivations return a fixed hash.
        // Non-Fixed-output derivations return a hash of the ATerm notation, but with all
        // input_derivation paths replaced by a recursive call to this function.
        // We use fn_get_derivation_or_fod_hash here, so callers can precompute this.
        let digest = self.fod_digest().unwrap_or({
            // This is a new map from derivation_or_fod_hash.digest (as lowerhex)
            // to list of output names
            let mut replaced_input_derivations: BTreeMap<String, BTreeSet<String>> =
                BTreeMap::new();

            // For each input_derivation, look up the
            // derivation_or_fod_hash, and replace the derivation path with it's HEXLOWER
            // digest.
            // This is not the [NixHash::to_nix_hash_string], but without the sha256: prefix).
            for (drv_path, output_names) in &self.input_derivations {
                replaced_input_derivations.insert(
                    data_encoding::HEXLOWER.encode(&fn_get_derivation_or_fod_hash(drv_path).digest),
                    output_names.clone(),
                );
            }

            // construct a new derivation struct with these replaced input derivation strings
            let replaced_derivation = Derivation {
                input_derivations: replaced_input_derivations,
                ..self.clone()
            };

            // write the ATerm of that to the hash function
            let mut hasher = Sha256::new();
            hasher.update(replaced_derivation.to_aterm_bytes());

            hasher.finalize().to_vec()
        });
        NixHash::new(crate::nixhash::HashAlgo::Sha256, digest.to_vec())
    }

    /// This calculates all output paths of a Derivation and updates the struct.
    /// It requires the struct to be initially without output paths.
    /// This means, self.outputs[$outputName].path needs to be an empty string,
    /// and self.environment[$outputName] needs to be an empty string.
    ///
    /// Output path calculation requires knowledge of the
    /// derivation_or_fod_hash [NixHash], which (in case of non-fixed-output
    /// derivations) also requires knowledge of other hash_derivation_modulo
    /// [NixHash]es.
    ///
    /// We solve this by asking the caller of this function to provide the
    /// hash_derivation_modulo of the current Derivation.
    ///
    /// On completion, self.environment[$outputName] and
    /// self.outputs[$outputName].path are set to the calculated output path for all
    /// outputs.
    pub fn calculate_output_paths(
        &mut self,
        name: &str,
        derivation_or_fod_hash: &NixHash,
    ) -> Result<(), DerivationError> {
        // The fingerprint and hash differs per output
        for (output_name, output) in self.outputs.iter_mut() {
            // Assert that outputs are not yet populated, to avoid using this function wrongly.
            // We don't also go over self.environment, but it's a sufficient
            // footgun prevention mechanism.
            assert!(output.path.is_empty());

            let path_name = output_path_name(name, output_name);

            // For fixed output derivation we use the per-output info, otherwise we use the
            // derivation hash.
            let abs_store_path = if let Some(ref hwm) = output.hash_with_mode {
                build_regular_ca_path(&path_name, hwm, Vec::<String>::new(), false).map_err(
                    |e| DerivationError::InvalidOutputDerivationPath(output_name.to_string(), e),
                )?
            } else {
                build_output_path(derivation_or_fod_hash, output_name, &path_name).map_err(|e| {
                    DerivationError::InvalidOutputDerivationPath(
                        output_name.to_string(),
                        store_path::BuildStorePathError::InvalidStorePath(e),
                    )
                })?
            };

            output.path = abs_store_path.to_absolute_path();
            self.environment.insert(
                output_name.to_string(),
                abs_store_path.to_absolute_path().into(),
            );
        }

        Ok(())
    }
}

/// Calculate the name part of the store path of a derivation [Output].
///
/// It's the name, and (if it's the non-out output), the output name
/// after a `-`.
fn output_path_name(derivation_name: &str, output_name: &str) -> String {
    let mut output_path_name = derivation_name.to_string();
    if output_name != "out" {
        output_path_name.push('-');
        output_path_name.push_str(output_name);
    }
    output_path_name
}
