use crate::{derivation::Derivation, write::DOT_FILE_EXT};
use anyhow::bail;
use tvix_store::nixpath::NixPath;

impl Derivation {
    /// validate ensures a Derivation struct is properly populated,
    /// and returns an error if not.
    /// TODO(flokli): make this proper errors
    pub fn validate(&self) -> anyhow::Result<()> {
        // Ensure the number of outputs is > 1
        if self.outputs.is_empty() {
            bail!("0 outputs");
        }

        // Validate all outputs
        for (output_name, output) in &self.outputs {
            if output_name.is_empty() {
                bail!("output_name from outputs may not be empty")
            }

            if output.is_fixed() {
                if self.outputs.len() != 1 {
                    bail!("encountered fixed-output, but there's more than 1 output in total");
                }
                if output_name != "out" {
                    bail!("the fixed-output output name must be called 'out'");
                }

                break;
            }

            output.validate()?;
        }

        // Validate all input_derivations
        for (input_derivation_path, output_names) in &self.input_derivations {
            // Validate input_derivation_path
            NixPath::from_absolute_path(input_derivation_path)?;
            if !input_derivation_path.ends_with(DOT_FILE_EXT) {
                bail!(
                    "derivation {} does not end with .drv",
                    input_derivation_path
                );
            }

            if output_names.is_empty() {
                bail!(
                    "output_names list for {} may not be empty",
                    input_derivation_path
                );
            }

            for (i, output_name) in output_names.iter().enumerate() {
                if output_name.is_empty() {
                    bail!(
                        "output name entry for {} may not be empty",
                        input_derivation_path
                    )
                }
                // if i is at least 1, peek at the previous element to ensure output_names are sorted.
                if i > 0 && (output_names[i - 1] >= *output_name) {
                    bail!(
                        "invalid input derivation output order: {} < {}",
                        output_name,
                        output_names[i - 1],
                    );
                }
            }
        }

        // Validate all input_sources
        for (i, input_source) in self.input_sources.iter().enumerate() {
            NixPath::from_absolute_path(input_source)?;

            if i > 0 && self.input_sources[i - 1] >= *input_source {
                bail!(
                    "invalid input source order: {} < {}",
                    input_source,
                    self.input_sources[i - 1],
                );
            }
        }

        // validate platform
        if self.system.is_empty() {
            bail!("required attribute 'platform' missing");
        }

        // validate builder
        if self.builder.is_empty() {
            bail!("required attribute 'builder' missing");
        }

        // validate env, none of the keys may be empty.
        // We skip the `name` validation seen in go-nix.
        for k in self.environment.keys() {
            if k.is_empty() {
                bail!("found empty environment variable key");
            }
        }

        Ok(())
    }
}
