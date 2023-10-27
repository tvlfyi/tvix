use crate::derivation::{Derivation, DerivationError};
use crate::store_path::{self, StorePath};

impl Derivation {
    /// validate ensures a Derivation struct is properly populated,
    /// and returns a [DerivationError] if not.
    ///
    /// if `validate_output_paths` is set to false, the output paths are
    /// excluded from validation.
    ///
    /// This is helpful to validate struct population before invoking
    /// [Derivation::calculate_output_paths].
    pub fn validate(&self, validate_output_paths: bool) -> Result<(), DerivationError> {
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
            // [StorePath] will fail the [store_path::validate_name] check.
            if output_name.is_empty()
                || output_name == "drv"
                || store_path::validate_name(output_name.as_bytes()).is_err()
            {
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
            }

            if let Err(e) = output.validate(validate_output_paths) {
                return Err(DerivationError::InvalidOutput(output_name.to_string(), e));
            }
        }

        // Validate all input_derivations
        for (input_derivation_path, output_names) in &self.input_derivations {
            // Validate input_derivation_path
            if let Err(e) = StorePath::from_absolute_path(input_derivation_path.as_bytes()) {
                return Err(DerivationError::InvalidInputDerivationPath(
                    input_derivation_path.to_string(),
                    e,
                ));
            }

            if !input_derivation_path.ends_with(".drv") {
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
                // Other output names that don't match the name restrictions from
                // [StorePath] will fail the [StorePath::validate_name] check.
                if output_name.is_empty()
                    || output_name == "drv"
                    || store_path::validate_name(output_name.as_bytes()).is_err()
                {
                    return Err(DerivationError::InvalidInputDerivationOutputName(
                        input_derivation_path.to_string(),
                        output_name.to_string(),
                    ));
                }
            }
        }

        // Validate all input_sources
        for input_source in self.input_sources.iter() {
            if let Err(e) = StorePath::from_absolute_path(input_source.as_bytes()) {
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

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use crate::derivation::{CAHash, Derivation, Output};

    /// Regression test: produce a Derivation that's almost valid, except its
    /// fixed-output output has the wrong hash specified.
    #[test]
    fn output_validate() {
        let mut outputs = BTreeMap::new();
        outputs.insert(
            "out".to_string(),
            Output {
                path: "".to_string(),
                ca_hash: Some(CAHash::Text([0; 32])), // This is disallowed
            },
        );

        let drv = Derivation {
            arguments: vec![],
            builder: "/bin/sh".to_string(),
            outputs,
            system: "x86_64-linux".to_string(),
            ..Default::default()
        };

        drv.validate(false).expect_err("must fail");
    }
}
