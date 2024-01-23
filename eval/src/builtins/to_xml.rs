//! This module implements `builtins.toXML`, which is a serialisation
//! of value information as well as internal tvix state that several
//! things in nixpkgs rely on.

use std::{io::Write, rc::Rc};
use xml::writer::events::XmlEvent;
use xml::writer::EmitterConfig;
use xml::writer::EventWriter;

use crate::{ErrorKind, Value};

/// Recursively serialise a value to XML. The value *must* have been
/// deep-forced before being passed to this function.
pub fn value_to_xml<W: Write>(mut writer: W, value: &Value) -> Result<(), ErrorKind> {
    let config = EmitterConfig {
        perform_indent: true,
        pad_self_closing: true,

        // Nix uses single-quotes *only* in the document declaration,
        // so we need to write it manually.
        write_document_declaration: false,
        ..Default::default()
    };

    // Write a literal document declaration, using C++-Nix-style
    // single quotes.
    writeln!(writer, "<?xml version='1.0' encoding='utf-8'?>")?;

    let mut writer = EventWriter::new_with_config(writer, config);

    writer.write(XmlEvent::start_element("expr"))?;
    value_variant_to_xml(&mut writer, value)?;
    writer.write(XmlEvent::end_element())?;

    // Unwrap the writer to add the final newline that C++ Nix adds.
    writeln!(writer.into_inner())?;

    Ok(())
}

fn write_typed_value<W: Write, V: ToString>(
    w: &mut EventWriter<W>,
    name: &str,
    value: V,
) -> Result<(), ErrorKind> {
    w.write(XmlEvent::start_element(name).attr("value", &value.to_string()))?;
    w.write(XmlEvent::end_element())?;
    Ok(())
}

fn value_variant_to_xml<W: Write>(w: &mut EventWriter<W>, value: &Value) -> Result<(), ErrorKind> {
    match value {
        Value::Thunk(t) => return value_variant_to_xml(w, &t.value()),

        Value::Null => {
            w.write(XmlEvent::start_element("null"))?;
            w.write(XmlEvent::end_element())
        }

        Value::Bool(b) => return write_typed_value(w, "bool", b),
        Value::Integer(i) => return write_typed_value(w, "int", i),
        Value::Float(f) => return write_typed_value(w, "float", f),
        Value::String(s) => return write_typed_value(w, "string", s.as_str()),
        Value::Path(p) => return write_typed_value(w, "path", p.to_string_lossy()),

        Value::List(list) => {
            w.write(XmlEvent::start_element("list"))?;

            for elem in list.into_iter() {
                value_variant_to_xml(w, elem)?;
            }

            w.write(XmlEvent::end_element())
        }

        Value::Attrs(attrs) => {
            w.write(XmlEvent::start_element("attrs"))?;

            for elem in attrs.iter() {
                w.write(XmlEvent::start_element("attr").attr("name", elem.0.as_str()))?;
                value_variant_to_xml(w, elem.1)?;
                w.write(XmlEvent::end_element())?;
            }

            w.write(XmlEvent::end_element())
        }

        Value::Closure(c) => {
            w.write(XmlEvent::start_element("function"))?;

            match &c.lambda.formals {
                Some(formals) => {
                    let mut attrspat = XmlEvent::start_element("attrspat");
                    if formals.ellipsis {
                        attrspat = attrspat.attr("ellipsis", "1");
                    }
                    if let Some(ref name) = &formals.name {
                        attrspat = attrspat.attr("name", name.as_str());
                    }

                    w.write(attrspat)?;

                    for arg in formals.arguments.iter() {
                        w.write(XmlEvent::start_element("attr").attr("name", arg.0.as_str()))?;
                        w.write(XmlEvent::end_element())?;
                    }

                    w.write(XmlEvent::end_element())?;
                }
                None => {
                    // TODO(tazjin): tvix does not currently persist function
                    // argument names anywhere (whereas we do for formals, as
                    // that is required for other runtime behaviour). Because of
                    // this the implementation here is fake, always returning
                    // the same argument name.
                    //
                    // If we don't want to persist the data, we can re-parse the
                    // AST from the spans of the lambda's bytecode and figure it
                    // out that way, but it needs some investigating.
                    w.write(XmlEvent::start_element("varpat").attr("name", /* fake: */ "x"))?;
                    w.write(XmlEvent::end_element())?;
                }
            }

            w.write(XmlEvent::end_element())
        }

        Value::Builtin(_) => {
            w.write(XmlEvent::start_element("unevaluated"))?;
            w.write(XmlEvent::end_element())
        }

        Value::AttrNotFound
        | Value::Blueprint(_)
        | Value::DeferredUpvalue(_)
        | Value::UnresolvedPath(_)
        | Value::Json(_)
        | Value::FinaliseRequest(_) => {
            return Err(ErrorKind::TvixBug {
                msg: "internal value variant encountered in builtins.toXML",
                metadata: Some(Rc::new(value.clone())),
            })
        }

        Value::Catchable(_) => {
            panic!("tvix bug: value_to_xml() called on a value which had not been deep-forced")
        }
    }?;

    Ok(())
}
