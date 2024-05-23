//! This module implements `builtins.toXML`, which is a serialisation
//! of value information as well as internal tvix state that several
//! things in nixpkgs rely on.

use bstr::ByteSlice;
use std::borrow::Cow;
use std::{io::Write, rc::Rc};

use crate::{ErrorKind, NixContext, NixContextElement, Value};

/// Recursively serialise a value to XML. The value *must* have been
/// deep-forced before being passed to this function.
/// On success, returns the NixContext.
pub fn value_to_xml<W: Write>(mut writer: W, value: &Value) -> Result<NixContext, ErrorKind> {
    // Write a literal document declaration, using C++-Nix-style
    // single quotes.
    writeln!(writer, "<?xml version='1.0' encoding='utf-8'?>")?;

    let mut emitter = XmlEmitter::new(writer);

    emitter.write_open_tag("expr", &[])?;
    value_variant_to_xml(&mut emitter, value)?;
    emitter.write_closing_tag("expr")?;

    Ok(emitter.into_context())
}

fn write_typed_value<W: Write, V: ToString>(
    w: &mut XmlEmitter<W>,
    name_unescaped: &str,
    value: V,
) -> Result<(), ErrorKind> {
    w.write_self_closing_tag(name_unescaped, &[("value", &value.to_string())])?;
    Ok(())
}

fn value_variant_to_xml<W: Write>(w: &mut XmlEmitter<W>, value: &Value) -> Result<(), ErrorKind> {
    match value {
        Value::Thunk(t) => return value_variant_to_xml(w, &t.value()),

        Value::Null => {
            w.write_open_tag("null", &[])?;
            w.write_closing_tag("null")?;
        }

        Value::Bool(b) => return write_typed_value(w, "bool", b),
        Value::Integer(i) => return write_typed_value(w, "int", i),
        Value::Float(f) => return write_typed_value(w, "float", f),
        Value::String(s) => {
            if let Some(context) = s.context() {
                w.extend_context(context.iter().cloned());
            }
            return write_typed_value(w, "string", s.to_str()?);
        }
        Value::Path(p) => return write_typed_value(w, "path", p.to_string_lossy()),

        Value::List(list) => {
            w.write_open_tag("list", &[])?;

            for elem in list.into_iter() {
                value_variant_to_xml(w, elem)?;
            }

            w.write_closing_tag("list")?;
        }

        Value::Attrs(attrs) => {
            w.write_open_tag("attrs", &[])?;

            for elem in attrs.iter() {
                w.write_open_tag("attr", &[("name", &elem.0.to_str_lossy())])?;
                value_variant_to_xml(w, elem.1)?;
                w.write_closing_tag("attr")?;
            }

            w.write_closing_tag("attrs")?;
        }

        Value::Closure(c) => {
            w.write_open_tag("function", &[])?;

            match &c.lambda.formals {
                Some(formals) => {
                    let mut attrs: Vec<(&str, &str)> = Vec::with_capacity(2);
                    if formals.ellipsis {
                        attrs.push(("ellipsis", "1"));
                    }
                    if let Some(ref name) = &formals.name {
                        attrs.push(("name", name.as_str()));
                    }

                    w.write_open_tag("attrspat", &attrs)?;
                    for arg in formals.arguments.iter() {
                        w.write_self_closing_tag("attr", &[("name", &arg.0.to_str_lossy())])?;
                    }

                    w.write_closing_tag("attrspat")?;
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
                    w.write_self_closing_tag("varpat", &[("name", /* fake: */ "x")])?;
                }
            }

            w.write_closing_tag("function")?;
        }

        Value::Builtin(_) => {
            w.write_open_tag("unevaluated", &[])?;
            w.write_closing_tag("unevaluated")?;
        }

        Value::AttrNotFound
        | Value::Blueprint(_)
        | Value::DeferredUpvalue(_)
        | Value::UnresolvedPath(_)
        | Value::Json(..)
        | Value::FinaliseRequest(_) => {
            return Err(ErrorKind::TvixBug {
                msg: "internal value variant encountered in builtins.toXML",
                metadata: Some(Rc::new(value.clone())),
            })
        }

        Value::Catchable(_) => {
            panic!("tvix bug: value_to_xml() called on a value which had not been deep-forced")
        }
    };

    Ok(())
}

/// A simple-stupid XML emitter, which implements only the subset needed for byte-by-byte compat with C++ nix’ `builtins.toXML`.
struct XmlEmitter<W> {
    /// The current indentation
    cur_indent: usize,
    writer: W,
    context: NixContext,
}

impl<W: Write> XmlEmitter<W> {
    pub fn new(writer: W) -> Self {
        XmlEmitter {
            cur_indent: 0,
            writer,
            context: Default::default(),
        }
    }

    /// Write an open tag with the given name (which is not escaped!)
    /// and attributes (Keys are not escaped! Only attribute values are.)
    pub fn write_open_tag(
        &mut self,
        name_unescaped: &str,
        attrs: &[(&str, &str)],
    ) -> std::io::Result<()> {
        self.add_indent()?;
        self.writer.write_all(b"<")?;
        self.writer.write_all(name_unescaped.as_bytes())?;
        self.write_attrs_escape_vals(attrs)?;
        self.writer.write_all(b">\n")?;
        self.cur_indent += 2;
        Ok(())
    }

    /// Write a self-closing open tag with the given name (which is not escaped!)
    /// and attributes (Keys are not escaped! Only attribute values are.)
    pub fn write_self_closing_tag(
        &mut self,
        name_unescaped: &str,
        attrs: &[(&str, &str)],
    ) -> std::io::Result<()> {
        self.add_indent()?;
        self.writer.write_all(b"<")?;
        self.writer.write_all(name_unescaped.as_bytes())?;
        self.write_attrs_escape_vals(attrs)?;
        self.writer.write_all(b" />\n")?;
        Ok(())
    }

    /// Write a closing tag with the given name (which is not escaped!)
    pub fn write_closing_tag(&mut self, name_unescaped: &str) -> std::io::Result<()> {
        self.cur_indent -= 2;
        self.add_indent()?;
        self.writer.write_all(b"</")?;
        self.writer.write_all(name_unescaped.as_bytes())?;
        self.writer.write_all(b">\n")?;
        Ok(())
    }

    #[inline]
    fn add_indent(&mut self) -> std::io::Result<()> {
        self.writer.write_all(&b" ".repeat(self.cur_indent))
    }

    /// Write an attribute list
    fn write_attrs_escape_vals(&mut self, attrs: &[(&str, &str)]) -> std::io::Result<()> {
        for (name, val) in attrs {
            self.writer.write_all(b" ")?;
            self.writer.write_all(name.as_bytes())?;
            self.writer.write_all(br#"=""#)?;
            self.writer
                .write_all(Self::escape_attr_value(val).as_bytes())?;
            self.writer.write_all(b"\"")?;
        }
        Ok(())
    }

    /// Escape the given attribute value, making sure we only actually clone the string if we needed to replace something.
    fn escape_attr_value(s: &str) -> Cow<str> {
        let mut last_escape: usize = 0;
        let mut res: Cow<str> = Cow::Borrowed("");
        // iterating via char_indices gives us the ability to index the original string slice at character boundaries
        for (idx, c) in s.char_indices() {
            match Self::should_escape_char(c) {
                None => {}
                Some(new) => {
                    // add characters since the last escape we did
                    res += &s[last_escape..idx];
                    // add the escaped value
                    res += new;
                    last_escape = idx + 1;
                }
            }
        }
        // we did not need to escape anything, so borrow original string
        if last_escape == 0 {
            Cow::Borrowed(s)
        } else {
            // add the remaining characters
            res += &s[last_escape..];
            res
        }
    }

    fn should_escape_char(c: char) -> Option<&'static str> {
        match c {
            '<' => Some("&lt;"),
            '>' => Some("&gt;"),
            '"' => Some("&quot;"),
            '\'' => Some("&apos;"),
            '&' => Some("&amp;"),
            '\n' => Some("&#xA;"),
            '\r' => Some("&#xD;"),
            _ => None,
        }
    }

    /// Extends the existing context with more context elements.
    fn extend_context<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = NixContextElement>,
    {
        self.context.extend(iter)
    }

    /// Consumes [Self] and returns the [NixContext] collected.
    fn into_context(self) -> NixContext {
        self.context
    }
}

#[cfg(test)]
mod tests {
    use bytes::buf::Writer;
    use pretty_assertions::assert_eq;

    use crate::builtins::to_xml::XmlEmitter;
    use std::borrow::Cow;

    #[test]
    fn xml_gen() {
        let mut buf = Vec::new();
        let mut x = XmlEmitter::new(&mut buf);
        x.write_open_tag("hello", &[("hi", "it’s me"), ("no", "<escape>")])
            .unwrap();
        x.write_self_closing_tag("self-closing", &[("tag", "yay")])
            .unwrap();
        x.write_closing_tag("hello").unwrap();

        assert_eq!(
            std::str::from_utf8(&buf).unwrap(),
            r##"<hello hi="it’s me" no="&lt;escape&gt;">
  <self-closing tag="yay" />
</hello>
"##
        );
    }

    #[test]
    fn xml_escape() {
        match XmlEmitter::<Writer<Vec<u8>>>::escape_attr_value("ab<>c&de") {
            Cow::Owned(s) => assert_eq!(s, "ab&lt;&gt;c&amp;de".to_string(), "escape stuff"),
            Cow::Borrowed(s) => panic!("s should be owned {}", s),
        }
        match XmlEmitter::<Writer<Vec<u8>>>::escape_attr_value("") {
            Cow::Borrowed(s) => assert_eq!(s, "", "empty escape is borrowed"),
            Cow::Owned(s) => panic!("s should be borrowed {}", s),
        }
        match XmlEmitter::<Writer<Vec<u8>>>::escape_attr_value("hi!ŷbla") {
            Cow::Borrowed(s) => assert_eq!(s, "hi!ŷbla", "no escape is borrowed"),
            Cow::Owned(s) => panic!("s should be borrowed {}", s),
        }
        match XmlEmitter::<Writer<Vec<u8>>>::escape_attr_value("hi!<ŷ>bla") {
            Cow::Owned(s) => assert_eq!(
                s,
                "hi!&lt;ŷ&gt;bla".to_string(),
                "multi-byte chars are correctly used"
            ),
            Cow::Borrowed(s) => panic!("s should be owned {}", s),
        }
    }
}
