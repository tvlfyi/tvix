//! This module implements the serialisation of derivations into the
//! [ATerm][] format used by C++ Nix.
//!
//! [ATerm]: http://program-transformation.org/Tools/ATermFormat.html

use crate::derivation::escape::escape_bstr;
use crate::derivation::output::Output;
use bstr::BString;
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    io::Error,
    io::Write,
};

pub const DERIVATION_PREFIX: &str = "Derive";
pub const PAREN_OPEN: char = '(';
pub const PAREN_CLOSE: char = ')';
pub const BRACKET_OPEN: char = '[';
pub const BRACKET_CLOSE: char = ']';
pub const COMMA: char = ',';
pub const QUOTE: char = '"';

// Writes a character to the writer.
pub(crate) fn write_char(writer: &mut impl Write, c: char) -> io::Result<()> {
    let mut buf = [0; 4];
    let b = c.encode_utf8(&mut buf).as_bytes();
    writer.write_all(b)
}

// Write a string `s` as a quoted field to the writer.
// The `escape` argument controls whether escaping will be skipped.
// This is the case if `s` is known to only contain characters that need no
// escaping.
pub(crate) fn write_field<S: AsRef<[u8]>>(
    writer: &mut impl Write,
    s: S,
    escape: bool,
) -> io::Result<()> {
    write_char(writer, QUOTE)?;

    if !escape {
        writer.write_all(s.as_ref())?;
    } else {
        writer.write_all(&escape_bstr(s.as_ref()))?;
    }

    write_char(writer, QUOTE)?;

    Ok(())
}

fn write_array_elements<S: AsRef<[u8]>>(
    writer: &mut impl Write,
    elements: &[S],
) -> Result<(), io::Error> {
    for (index, element) in elements.iter().enumerate() {
        if index > 0 {
            write_char(writer, COMMA)?;
        }

        write_field(writer, element, true)?;
    }

    Ok(())
}

pub fn write_outputs(
    writer: &mut impl Write,
    outputs: &BTreeMap<String, Output>,
) -> Result<(), io::Error> {
    write_char(writer, BRACKET_OPEN)?;
    for (ii, (output_name, output)) in outputs.iter().enumerate() {
        if ii > 0 {
            write_char(writer, COMMA)?;
        }

        write_char(writer, PAREN_OPEN)?;

        let mut elements: Vec<&str> = vec![output_name, &output.path];

        let (e2, e3) = match &output.hash_with_mode {
            Some(hash) => match hash {
                crate::nixhash::NixHashWithMode::Flat(h) => (
                    h.algo.to_string(),
                    data_encoding::HEXLOWER.encode(&h.digest),
                ),
                crate::nixhash::NixHashWithMode::Recursive(h) => (
                    format!("r:{}", h.algo),
                    data_encoding::HEXLOWER.encode(&h.digest),
                ),
            },
            None => ("".to_string(), "".to_string()),
        };

        elements.push(&e2);
        elements.push(&e3);

        write_array_elements(writer, &elements)?;

        write_char(writer, PAREN_CLOSE)?;
    }
    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_input_derivations(
    writer: &mut impl Write,
    input_derivations: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), io::Error> {
    write_char(writer, BRACKET_OPEN)?;

    for (ii, (input_derivation_path, input_derivation)) in input_derivations.into_iter().enumerate()
    {
        if ii > 0 {
            write_char(writer, COMMA)?;
        }

        write_char(writer, PAREN_OPEN)?;
        write_field(writer, input_derivation_path.as_str(), false)?;
        write_char(writer, COMMA)?;

        write_char(writer, BRACKET_OPEN)?;
        write_array_elements(
            writer,
            &input_derivation
                .iter()
                .map(|s| s.as_bytes().to_vec().into())
                .collect::<Vec<BString>>(),
        )?;
        write_char(writer, BRACKET_CLOSE)?;

        write_char(writer, PAREN_CLOSE)?;
    }

    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_input_sources(
    writer: &mut impl Write,
    input_sources: &BTreeSet<String>,
) -> Result<(), io::Error> {
    write_char(writer, BRACKET_OPEN)?;
    write_array_elements(
        writer,
        &input_sources
            .iter()
            .map(|s| s.as_bytes().to_vec().into())
            .collect::<Vec<BString>>(),
    )?;
    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_system(writer: &mut impl Write, platform: &str) -> Result<(), Error> {
    write_field(writer, platform, true)?;
    Ok(())
}

pub fn write_builder(writer: &mut impl Write, builder: &str) -> Result<(), Error> {
    write_field(writer, builder, true)?;
    Ok(())
}

pub fn write_arguments(writer: &mut impl Write, arguments: &[String]) -> Result<(), io::Error> {
    write_char(writer, BRACKET_OPEN)?;
    write_array_elements(
        writer,
        &arguments
            .iter()
            .map(|s| s.as_bytes().to_vec().into())
            .collect::<Vec<BString>>(),
    )?;
    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_enviroment(
    writer: &mut impl Write,
    environment: &BTreeMap<String, BString>,
) -> Result<(), io::Error> {
    write_char(writer, BRACKET_OPEN)?;

    for (i, (k, v)) in environment.into_iter().enumerate() {
        if i > 0 {
            write_char(writer, COMMA)?;
        }

        write_char(writer, PAREN_OPEN)?;
        write_field(writer, k, false)?;
        write_char(writer, COMMA)?;
        write_field(writer, v, true)?;
        write_char(writer, PAREN_CLOSE)?;
    }

    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}
