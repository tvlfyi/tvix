use crate::{derivation::Derivation, write::DOT_FILE_EXT, DerivationError};
use tvix_store::store_path::StorePath;

impl Derivation {
    /// validate ensures a Derivation struct is properly populated,
    /// and returns a [ValidateDerivationError] if not.
    pub fn validate(&self) -> Result<(), DerivationError> {
        // Ensure the number of outputs is > 1
        if self.outputs.is_empty() {
            return Err(DerivationError::NoOutputs());
        }

        // Validate all outputs
        for (output_name, output) in &self.outputs {
            // empty output names are invalid.
            //
            // `drv` is an invalid output name too, as this would cause
            // a `builtins.derivation` call to return an attrset with a
            // `drvPath` key (which already exists) and has a different
            // meaning.
            //
            // Other output names that don't match the name restrictions from
            // [StorePath] will fail output path calculation.
            if output_name.is_empty() || output_name == "drv" {
                return Err(DerivationError::InvalidOutputName(output_name.to_string()));
            }

            if output.is_fixed() {
                if self.outputs.len() != 1 {
                    return Err(DerivationError::MoreThanOneOutputButFixed());
                }
                if output_name != "out" {
                    return Err(DerivationError::InvalidOutputNameForFixed(
                        output_name.to_string(),
                    ));
                }

                break;
            }

            if let Err(e) = output.validate() {
                return Err(DerivationError::InvalidOutput(output_name.to_string(), e));
            }
        }

        // Validate all input_derivations
        for (input_derivation_path, output_names) in &self.input_derivations {
            // Validate input_derivation_path
            if let Err(e) = StorePath::from_absolute_path(input_derivation_path) {
                return Err(DerivationError::InvalidInputDerivationPath(
                    input_derivation_path.to_string(),
                    e,
                ));
            }

            if !input_derivation_path.ends_with(DOT_FILE_EXT) {
                return Err(DerivationError::InvalidInputDerivationPrefix(
                    input_derivation_path.to_string(),
                ));
            }

            if output_names.is_empty() {
                return Err(DerivationError::EmptyInputDerivationOutputNames(
                    input_derivation_path.to_string(),
                ));
            }

            for output_name in output_names.iter() {
                // empty output names are invalid.
                //
                // `drv` is an invalid output name too, as this would cause
                // a `builtins.derivation` call to return an attrset with a
                // `drvPath` key (which already exists) and has a different
                // meaning.
                //
                // Other output names that don't match the name restrictions
                // from [StorePath] can't be constructed with this library, but
                // are not explicitly checked here (yet).
                if output_name.is_empty() || output_name == "drv" {
                    return Err(DerivationError::InvalidInputDerivationOutputName(
                        input_derivation_path.to_string(),
                        output_name.to_string(),
                    ));
                }
            }
        }

        // Validate all input_sources
        for input_source in self.input_sources.iter() {
            if let Err(e) = StorePath::from_absolute_path(input_source) {
                return Err(DerivationError::InvalidInputSourcesPath(
                    input_source.to_string(),
                    e,
                ));
            }
        }

        // validate platform
        if self.system.is_empty() {
            return Err(DerivationError::InvalidPlatform(self.system.to_string()));
        }

        // validate builder
        if self.builder.is_empty() {
            return Err(DerivationError::InvalidBuilder(self.builder.to_string()));
        }

        // validate env, none of the keys may be empty.
        // We skip the `name` validation seen in go-nix.
        for k in self.environment.keys() {
            if k.is_empty() {
                return Err(DerivationError::InvalidEnvironmentKey(k.to_string()));
            }
        }

        Ok(())
    }
}
