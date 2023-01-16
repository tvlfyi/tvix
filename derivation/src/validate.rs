use crate::{derivation::Derivation, write::DOT_FILE_EXT, ValidateDerivationError};
use tvix_store::store_path::StorePath;

impl Derivation {
    /// validate ensures a Derivation struct is properly populated,
    /// and returns a [ValidateDerivationError] if not.
    pub fn validate(&self) -> Result<(), ValidateDerivationError> {
        // Ensure the number of outputs is > 1
        if self.outputs.is_empty() {
            return Err(ValidateDerivationError::NoOutputs());
        }

        // Validate all outputs
        for (output_name, output) in &self.outputs {
            if output_name.is_empty() {
                return Err(ValidateDerivationError::InvalidOutputName(
                    output_name.to_string(),
                ));
            }

            if output.is_fixed() {
                if self.outputs.len() != 1 {
                    return Err(ValidateDerivationError::MoreThanOneOutputButFixed());
                }
                if output_name != "out" {
                    return Err(ValidateDerivationError::InvalidOutputNameForFixed(
                        output_name.to_string(),
                    ));
                }

                break;
            }

            if let Err(e) = output.validate() {
                return Err(ValidateDerivationError::InvalidOutputPath(
                    output_name.to_string(),
                    e,
                ));
            };
        }

        // Validate all input_derivations
        for (input_derivation_path, output_names) in &self.input_derivations {
            // Validate input_derivation_path
            if let Err(e) = StorePath::from_absolute_path(input_derivation_path) {
                return Err(ValidateDerivationError::InvalidInputDerivationPath(
                    input_derivation_path.to_string(),
                    e,
                ));
            }

            if !input_derivation_path.ends_with(DOT_FILE_EXT) {
                return Err(ValidateDerivationError::InvalidInputDerivationPrefix(
                    input_derivation_path.to_string(),
                ));
            }

            if output_names.is_empty() {
                return Err(ValidateDerivationError::EmptyInputDerivationOutputNames(
                    input_derivation_path.to_string(),
                ));
            }

            for output_name in output_names.iter() {
                if output_name.is_empty() {
                    return Err(ValidateDerivationError::InvalidInputDerivationOutputName(
                        input_derivation_path.to_string(),
                        output_name.to_string(),
                    ));
                }
                // TODO: do we need to apply more name validation here?
            }
        }

        // Validate all input_sources
        for input_source in self.input_sources.iter() {
            if let Err(e) = StorePath::from_absolute_path(input_source) {
                return Err(ValidateDerivationError::InvalidInputSourcesPath(
                    input_source.to_string(),
                    e,
                ));
            }
        }

        // validate platform
        if self.system.is_empty() {
            return Err(ValidateDerivationError::InvalidPlatform(
                self.system.to_string(),
            ));
        }

        // validate builder
        if self.builder.is_empty() {
            return Err(ValidateDerivationError::InvalidBuilder(
                self.builder.to_string(),
            ));
        }

        // validate env, none of the keys may be empty.
        // We skip the `name` validation seen in go-nix.
        for k in self.environment.keys() {
            if k.is_empty() {
                return Err(ValidateDerivationError::InvalidEnvironmentKey(
                    k.to_string(),
                ));
            }
        }

        Ok(())
    }
}
