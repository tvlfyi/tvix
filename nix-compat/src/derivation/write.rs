//! This module implements the serialisation of derivations into the
//! [ATerm][] format used by C++ Nix.
//!
//! [ATerm]: http://program-transformation.org/Tools/ATermFormat.html

use crate::derivation::escape::escape_bstr;
use crate::derivation::output::Output;
use bstr::BString;
use std::collections::BTreeSet;
use std::io::Cursor;
use std::{collections::BTreeMap, io, io::Error, io::Write};

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
    io::copy(&mut Cursor::new(b), writer)?;
    Ok(())
}

// Writes a string to the writer (as unicode)
pub(crate) fn write_str(writer: &mut impl Write, s: &str) -> io::Result<()> {
    io::copy(&mut Cursor::new(s.as_bytes()), writer)?;
    Ok(())
}

fn write_array_elements(
    writer: &mut impl Write,
    quote: bool,
    open: &str,
    closing: &str,
    elements: &[BString],
) -> Result<(), io::Error> {
    write_str(writer, open)?;

    for (index, element) in elements.iter().enumerate() {
        if index > 0 {
            write_char(writer, COMMA)?;
        }

        if quote {
            write_char(writer, QUOTE)?;
        }

        io::copy(&mut Cursor::new(element), writer)?;

        if quote {
            write_char(writer, QUOTE)?;
        }
    }

    write_str(writer, closing)?;

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

        let mut elements: Vec<BString> = vec![
            output_name.as_bytes().to_vec().into(),
            output.path.as_bytes().to_vec().into(),
        ];

        let (e2, e3) = match &output.hash_with_mode {
            Some(hash) => match hash {
                crate::nixhash::NixHashWithMode::Flat(h) => (
                    h.algo.to_string().as_bytes().to_vec(),
                    data_encoding::HEXLOWER.encode(&h.digest).as_bytes().into(),
                ),
                crate::nixhash::NixHashWithMode::Recursive(h) => (
                    format!("r:{}", h.algo).as_bytes().to_vec(),
                    data_encoding::HEXLOWER.encode(&h.digest).as_bytes().into(),
                ),
            },
            None => (vec![], vec![]),
        };

        elements.push(e2.into());
        elements.push(e3.into());

        write_array_elements(
            writer,
            true,
            &PAREN_OPEN.to_string(),
            &PAREN_CLOSE.to_string(),
            &elements,
        )?
    }
    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_input_derivations(
    writer: &mut impl Write,
    input_derivations: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), io::Error> {
    write_char(writer, COMMA)?;
    write_char(writer, BRACKET_OPEN)?;

    for (ii, (input_derivation_path, input_derivation)) in input_derivations.into_iter().enumerate()
    {
        if ii > 0 {
            write_char(writer, COMMA)?;
        }

        write_char(writer, PAREN_OPEN)?;
        write_char(writer, QUOTE)?;
        write_str(writer, input_derivation_path.as_str())?;
        write_char(writer, QUOTE)?;
        write_char(writer, COMMA)?;

        write_array_elements(
            writer,
            true,
            &BRACKET_OPEN.to_string(),
            &BRACKET_CLOSE.to_string(),
            &input_derivation
                .iter()
                .map(|s| s.as_bytes().to_vec().into())
                .collect::<Vec<BString>>(),
        )?;

        write_char(writer, PAREN_CLOSE)?;
    }

    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}

pub fn write_input_sources(
    writer: &mut impl Write,
    input_sources: &BTreeSet<String>,
) -> Result<(), io::Error> {
    write_char(writer, COMMA)?;

    write_array_elements(
        writer,
        true,
        &BRACKET_OPEN.to_string(),
        &BRACKET_CLOSE.to_string(),
        &input_sources
            .iter()
            .map(|s| s.as_bytes().to_vec().into())
            .collect::<Vec<BString>>(),
    )?;

    Ok(())
}

pub fn write_system(writer: &mut impl Write, platform: &str) -> Result<(), Error> {
    write_char(writer, COMMA)?;
    io::copy(&mut Cursor::new(escape_bstr(platform.as_bytes())), writer)?;
    Ok(())
}

pub fn write_builder(writer: &mut impl Write, builder: &str) -> Result<(), Error> {
    write_char(writer, COMMA)?;
    io::copy(&mut Cursor::new(escape_bstr(builder.as_bytes())), writer)?;
    Ok(())
}
pub fn write_arguments(writer: &mut impl Write, arguments: &[String]) -> Result<(), io::Error> {
    write_char(writer, COMMA)?;
    write_array_elements(
        writer,
        true,
        &BRACKET_OPEN.to_string(),
        &BRACKET_CLOSE.to_string(),
        &arguments
            .iter()
            .map(|s| s.as_bytes().to_vec().into())
            .collect::<Vec<BString>>(),
    )?;

    Ok(())
}

pub fn write_enviroment(
    writer: &mut impl Write,
    environment: &BTreeMap<String, BString>,
) -> Result<(), io::Error> {
    write_char(writer, COMMA)?;
    write_char(writer, BRACKET_OPEN)?;

    for (i, (k, v)) in environment.into_iter().enumerate() {
        if i > 0 {
            write_char(writer, COMMA)?;
        }

        write_array_elements(
            writer,
            false,
            &PAREN_OPEN.to_string(),
            &PAREN_CLOSE.to_string(),
            &[escape_bstr(k.as_bytes()), escape_bstr(v)],
        )?;
    }

    write_char(writer, BRACKET_CLOSE)?;

    Ok(())
}
