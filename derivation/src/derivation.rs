use crate::output::Output;
use crate::write;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, fmt::Write};

#[derive(Serialize, Deserialize)]
pub struct Derivation {
    outputs: BTreeMap<String, Output>,
    input_sources: Vec<String>,
    input_derivations: BTreeMap<String, Vec<String>>,
    platform: String,
    builder: String,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
}

impl Derivation {
    pub fn serialize(&self, writer: &mut impl Write) -> Result<(), fmt::Error> {
        writer.write_str(write::DERIVATION_PREFIX)?;
        writer.write_char(write::PAREN_OPEN)?;

        write::write_outputs(writer, &self.outputs)?;
        write::write_input_derivations(writer, &self.input_derivations)?;
        write::write_input_sources(writer, &self.input_sources)?;
        write::write_platfrom(writer, &self.platform)?;
        write::write_builder(writer, &self.builder)?;
        write::write_arguments(writer, &self.arguments)?;
        write::write_enviroment(writer, &self.environment)?;

        writer.write_char(write::PAREN_CLOSE)?;

        Ok(())
    }
}

impl fmt::Display for Derivation {
    /// Formats the Derivation in ATerm representation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.serialize(f)
    }
}
