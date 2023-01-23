//! This module implements the serialisation of derivations into the
//! [ATerm][] format used by C++ Nix.
//!
//! [ATerm]: http://program-transformation.org/Tools/ATermFormat.html

use crate::output::Output;
use crate::string_escape::escape_string;
use std::collections::BTreeSet;
use std::{collections::BTreeMap, fmt, fmt::Write};

pub const DERIVATION_PREFIX: &str = "Derive";
pub const PAREN_OPEN: char = '(';
pub const PAREN_CLOSE: char = ')';
pub const BRACKET_OPEN: char = '[';
pub const BRACKET_CLOSE: char = ']';
pub const COMMA: char = ',';
pub const QUOTE: char = '"';

pub const DOT_FILE_EXT: &str = ".drv";

fn write_array_elements(
    writer: &mut impl Write,
    quote: bool,
    open: &str,
    closing: &str,
    elements: Vec<&str>,
) -> Result<(), fmt::Error> {
    writer.write_str(open)?;

    for (index, element) in elements.iter().enumerate() {
        if index > 0 {
            writer.write_char(COMMA)?;
        }

        if quote {
            writer.write_char(QUOTE)?;
        }

        writer.write_str(element)?;

        if quote {
            writer.write_char(QUOTE)?;
        }
    }

    writer.write_str(closing)?;

    Ok(())
}

pub fn write_outputs(
    writer: &mut impl Write,
    outputs: &BTreeMap<String, Output>,
) -> Result<(), fmt::Error> {
    writer.write_char(BRACKET_OPEN)?;
    for (ii, (output_name, output)) in outputs.iter().enumerate() {
        if ii > 0 {
            writer.write_char(COMMA)?;
        }

        let mut elements: Vec<&str> = vec![output_name, &output.path];

        match &output.hash {
            Some(hash) => {
                elements.push(&hash.algo);
                elements.push(&hash.digest);
            }
            None => {
                elements.push("");
                elements.push("");
            }
        }

        write_array_elements(
            writer,
            true,
            &PAREN_OPEN.to_string(),
            &PAREN_CLOSE.to_string(),
            elements,
        )?
    }
    writer.write_char(BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_input_derivations(
    writer: &mut impl Write,
    input_derivations: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), fmt::Error> {
    writer.write_char(COMMA)?;
    writer.write_char(BRACKET_OPEN)?;

    for (ii, (input_derivation_path, input_derivation)) in input_derivations.iter().enumerate() {
        if ii > 0 {
            writer.write_char(COMMA)?;
        }

        writer.write_char(PAREN_OPEN)?;
        writer.write_char(QUOTE)?;
        writer.write_str(input_derivation_path.as_str())?;
        writer.write_char(QUOTE)?;
        writer.write_char(COMMA)?;

        write_array_elements(
            writer,
            true,
            &BRACKET_OPEN.to_string(),
            &BRACKET_CLOSE.to_string(),
            input_derivation.iter().map(|s| &**s).collect(),
        )?;

        writer.write_char(PAREN_CLOSE)?;
    }

    writer.write_char(BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_input_sources(
    writer: &mut impl Write,
    input_sources: &BTreeSet<String>,
) -> Result<(), fmt::Error> {
    writer.write_char(COMMA)?;

    write_array_elements(
        writer,
        true,
        &BRACKET_OPEN.to_string(),
        &BRACKET_CLOSE.to_string(),
        input_sources.iter().map(|s| &**s).collect(),
    )?;

    Ok(())
}

pub fn write_system(writer: &mut impl Write, platform: &str) -> Result<(), fmt::Error> {
    writer.write_char(COMMA)?;
    writer.write_str(escape_string(platform).as_str())?;
    Ok(())
}

pub fn write_builder(writer: &mut impl Write, builder: &str) -> Result<(), fmt::Error> {
    writer.write_char(COMMA)?;
    writer.write_str(escape_string(builder).as_str())?;
    Ok(())
}
pub fn write_arguments(writer: &mut impl Write, arguments: &[String]) -> Result<(), fmt::Error> {
    writer.write_char(COMMA)?;
    write_array_elements(
        writer,
        true,
        &BRACKET_OPEN.to_string(),
        &BRACKET_CLOSE.to_string(),
        arguments.iter().map(|s| &**s).collect(),
    )?;

    Ok(())
}

pub fn write_enviroment(
    writer: &mut impl Write,
    environment: &BTreeMap<String, String>,
) -> Result<(), fmt::Error> {
    writer.write_char(COMMA)?;
    writer.write_char(BRACKET_OPEN)?;

    for (ii, (key, environment)) in environment.iter().enumerate() {
        if ii > 0 {
            writer.write_char(COMMA)?;
        }

        write_array_elements(
            writer,
            false,
            &PAREN_OPEN.to_string(),
            &PAREN_CLOSE.to_string(),
            vec![&escape_string(key), &escape_string(environment)],
        )?;
    }

    writer.write_char(BRACKET_CLOSE)?;

    Ok(())
}
