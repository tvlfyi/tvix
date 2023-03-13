//! This module implements the backing representation of runtime
//! values in the Nix language.
use std::cmp::Ordering;
use std::fmt::Display;
use std::num::{NonZeroI32, NonZeroUsize};
use std::path::PathBuf;
use std::rc::Rc;

use lexical_core::format::CXX_LITERAL;
use serde::Deserialize;

#[cfg(feature = "arbitrary")]
mod arbitrary;
mod attrs;
mod builtin;
mod function;
mod json;
mod list;
mod path;
mod string;
mod thunk;

use crate::errors::ErrorKind;
use crate::opcode::StackIdx;
use crate::vm::generators::{self, GenCo};
use crate::AddContext;
pub use attrs::NixAttrs;
pub use builtin::{Builtin, BuiltinResult};
pub(crate) use function::Formals;
pub use function::{Closure, Lambda};
pub use list::NixList;
pub use path::canon_path;
pub use string::NixString;
pub use thunk::Thunk;

pub use self::thunk::{SharedThunkSet, ThunkSet};

use lazy_static::lazy_static;

#[warn(variant_size_differences)]
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(NixString),

    #[serde(skip)]
    Path(Box<PathBuf>),
    Attrs(Box<NixAttrs>),
    List(NixList),

    #[serde(skip)]
    Closure(Rc<Closure>), // must use Rc<Closure> here in order to get proper pointer equality

    #[serde(skip)]
    Builtin(Builtin),

    // Internal values that, while they technically exist at runtime,
    // are never returned to or created directly by users.
    #[serde(skip_deserializing)]
    Thunk(Thunk),

    // See [`compiler::compile_select_or()`] for explanation
    #[serde(skip)]
    AttrNotFound,

    // this can only occur in Chunk::Constants and nowhere else
    #[serde(skip)]
    Blueprint(Rc<Lambda>),

    #[serde(skip)]
    DeferredUpvalue(StackIdx),
    #[serde(skip)]
    UnresolvedPath(Box<PathBuf>),
    #[serde(skip)]
    Json(serde_json::Value),
}

lazy_static! {
    static ref WRITE_FLOAT_OPTIONS: lexical_core::WriteFloatOptions =
        lexical_core::WriteFloatOptionsBuilder::new()
            .trim_floats(true)
            .round_mode(lexical_core::write_float_options::RoundMode::Round)
            .positive_exponent_break(Some(NonZeroI32::new(5).unwrap()))
            .max_significant_digits(Some(NonZeroUsize::new(6).unwrap()))
            .build()
            .unwrap();
}

// Helper macros to generate the to_*/as_* macros while accounting for
// thunks.

/// Generate an `as_*` method returning a reference to the expected
/// type, or a type error. This only works for types that implement
/// `Copy`, as returning a reference to an inner thunk value is not
/// possible.

/// Generate an `as_*/to_*` accessor method that returns either the
/// expected type, or a type error.
macro_rules! gen_cast {
    ( $name:ident, $type:ty, $expected:expr, $variant:pat, $result:expr ) => {
        pub fn $name(&self) -> Result<$type, ErrorKind> {
            match self {
                $variant => Ok($result),
                Value::Thunk(thunk) => Self::$name(&thunk.value()),
                other => Err(type_error($expected, &other)),
            }
        }
    };
}

/// Generate an `as_*_mut/to_*_mut` accessor method that returns either the
/// expected type, or a type error.
macro_rules! gen_cast_mut {
    ( $name:ident, $type:ty, $expected:expr, $variant:ident) => {
        pub fn $name(&mut self) -> Result<&mut $type, ErrorKind> {
            match self {
                Value::$variant(x) => Ok(x),
                other => Err(type_error($expected, &other)),
            }
        }
    };
}

/// Generate an `is_*` type-checking method.
macro_rules! gen_is {
    ( $name:ident, $variant:pat ) => {
        pub fn $name(&self) -> bool {
            match self {
                $variant => true,
                Value::Thunk(thunk) => Self::$name(&thunk.value()),
                _ => false,
            }
        }
    };
}

/// Describes what input types are allowed when coercing a `Value` to a string
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CoercionKind {
    /// Only coerce already "stringly" types like strings and paths, but also
    /// coerce sets that have a `__toString` attribute. Equivalent to
    /// `!coerceMore` in C++ Nix.
    Weak,
    /// Coerce all value types included by `Weak`, but also coerce `null`,
    /// booleans, integers, floats and lists of coercible types. Equivalent to
    /// `coerceMore` in C++ Nix.
    Strong,
}

impl<T> From<T> for Value
where
    T: Into<NixString>,
{
    fn from(t: T) -> Self {
        Self::String(t.into())
    }
}

/// Constructors
impl Value {
    /// Construct a [`Value::Attrs`] from a [`NixAttrs`].
    pub fn attrs(attrs: NixAttrs) -> Self {
        Self::Attrs(Box::new(attrs))
    }
}

/// Controls what kind of by-pointer equality comparison is allowed.
///
/// See `//tvix/docs/value-pointer-equality.md` for details.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PointerEquality {
    /// Pointer equality not allowed at all.
    ForbidAll,

    /// Pointer equality comparisons only allowed for nested values.
    AllowNested,

    /// Pointer equality comparisons are allowed in all contexts.
    AllowAll,
}

impl Value {
    /// Deeply forces a value, traversing e.g. lists and attribute sets and forcing
    /// their contents, too.
    ///
    /// This is a generator function.
    pub(super) async fn deep_force(
        self,
        co: GenCo,
        thunk_set: SharedThunkSet,
    ) -> Result<Value, ErrorKind> {
        // Get rid of any top-level thunks, and bail out of self-recursive
        // thunks.
        let value = if let Value::Thunk(ref t) = &self {
            if !thunk_set.insert(t) {
                return Ok(self);
            }
            generators::request_force(&co, self).await
        } else {
            self
        };

        match &value {
            // Short-circuit on already evaluated values, or fail on internal values.
            Value::Null
            | Value::Bool(_)
            | Value::Integer(_)
            | Value::Float(_)
            | Value::String(_)
            | Value::Path(_)
            | Value::Closure(_)
            | Value::Builtin(_) => return Ok(value),

            Value::List(list) => {
                for val in list {
                    generators::request_deep_force(&co, val.clone(), thunk_set.clone()).await;
                }
            }

            Value::Attrs(attrs) => {
                for (_, val) in attrs.iter() {
                    generators::request_deep_force(&co, val.clone(), thunk_set.clone()).await;
                }
            }

            Value::Thunk(_) => panic!("Tvix bug: force_value() returned a thunk"),

            Value::AttrNotFound
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_)
            | Value::UnresolvedPath(_)
            | Value::Json(_) => panic!(
                "Tvix bug: internal value left on stack: {}",
                value.type_of()
            ),
        };

        Ok(value)
    }

    /// Coerce a `Value` to a string. See `CoercionKind` for a rundown of what
    /// input types are accepted under what circumstances.
    pub async fn coerce_to_string(self, co: GenCo, kind: CoercionKind) -> Result<Value, ErrorKind> {
        let value = generators::request_force(&co, self).await;

        match (value, kind) {
            // coercions that are always done
            tuple @ (Value::String(_), _) => Ok(tuple.0),

            // TODO(sterni): Think about proper encoding handling here. This needs
            // general consideration anyways, since one current discrepancy between
            // C++ Nix and Tvix is that the former's strings are arbitrary byte
            // sequences without NUL bytes, whereas Tvix only allows valid
            // Unicode. See also b/189.
            (Value::Path(p), _) => {
                // TODO(tazjin): there are cases where coerce_to_string does not import
                let imported = generators::request_path_import(&co, *p).await;
                Ok(imported.to_string_lossy().into_owned().into())
            }

            // Attribute sets can be converted to strings if they either have an
            // `__toString` attribute which holds a function that receives the
            // set itself or an `outPath` attribute which should be a string.
            // `__toString` is preferred.
            (Value::Attrs(attrs), kind) => {
                if let Some(s) = attrs.try_to_string(&co, kind).await {
                    return Ok(Value::String(s));
                }

                if let Some(out_path) = attrs.select("outPath") {
                    let s = generators::request_string_coerce(&co, out_path.clone(), kind).await;
                    return Ok(Value::String(s));
                }

                Err(ErrorKind::NotCoercibleToString { from: "set", kind })
            }

            // strong coercions
            (Value::Null, CoercionKind::Strong) | (Value::Bool(false), CoercionKind::Strong) => {
                Ok("".into())
            }
            (Value::Bool(true), CoercionKind::Strong) => Ok("1".into()),

            (Value::Integer(i), CoercionKind::Strong) => Ok(format!("{i}").into()),
            (Value::Float(f), CoercionKind::Strong) => {
                // contrary to normal Display, coercing a float to a string will
                // result in unconditional 6 decimal places
                Ok(format!("{:.6}", f).into())
            }

            // Lists are coerced by coercing their elements and interspersing spaces
            (Value::List(list), CoercionKind::Strong) => {
                let mut out = String::new();

                for (idx, elem) in list.into_iter().enumerate() {
                    if idx > 0 {
                        out.push(' ');
                    }

                    let s = generators::request_string_coerce(&co, elem, kind).await;
                    out.push_str(s.as_str());
                }

                Ok(Value::String(out.into()))
            }

            (Value::Thunk(_), _) => panic!("Tvix bug: force returned unforced thunk"),

            val @ (Value::Closure(_), _)
            | val @ (Value::Builtin(_), _)
            | val @ (Value::Null, _)
            | val @ (Value::Bool(_), _)
            | val @ (Value::Integer(_), _)
            | val @ (Value::Float(_), _)
            | val @ (Value::List(_), _) => Err(ErrorKind::NotCoercibleToString {
                from: val.0.type_of(),
                kind,
            }),

            (Value::AttrNotFound, _)
            | (Value::Blueprint(_), _)
            | (Value::DeferredUpvalue(_), _)
            | (Value::UnresolvedPath(_), _)
            | (Value::Json(_), _) => {
                panic!("tvix bug: .coerce_to_string() called on internal value")
            }
        }
    }

    /// Compare two Nix values for equality, forcing nested parts of the structure
    /// as needed.
    ///
    /// This comparison needs to be invoked for nested values (e.g. in lists and
    /// attribute sets) as well, which is done by suspending and asking the VM to
    /// perform the nested comparison.
    ///
    /// The `top_level` parameter controls whether this invocation is the top-level
    /// comparison, or a nested value comparison. See
    /// `//tvix/docs/value-pointer-equality.md`
    pub(crate) async fn nix_eq(
        self,
        other: Value,
        co: GenCo,
        ptr_eq: PointerEquality,
    ) -> Result<Value, ErrorKind> {
        let a = match self {
            Value::Thunk(ref thunk) => {
                // If both values are thunks, and thunk comparisons are allowed by
                // pointer, do that and move on.
                if ptr_eq == PointerEquality::AllowAll {
                    if let Value::Thunk(t1) = &other {
                        if t1.ptr_eq(thunk) {
                            return Ok(Value::Bool(true));
                        }
                    }
                };

                generators::request_force(&co, self).await
            }

            _ => self,
        };

        let b = match other {
            Value::Thunk(_) => generators::request_force(&co, other).await,
            _ => other,
        };

        debug_assert!(!matches!(a, Value::Thunk(_)));
        debug_assert!(!matches!(b, Value::Thunk(_)));

        let result = match (a, b) {
            // Trivial comparisons
            (Value::Null, Value::Null) => true,
            (Value::Bool(b1), Value::Bool(b2)) => b1 == b2,
            (Value::String(s1), Value::String(s2)) => s1 == s2,
            (Value::Path(p1), Value::Path(p2)) => p1 == p2,

            // Numerical comparisons (they work between float & int)
            (Value::Integer(i1), Value::Integer(i2)) => i1 == i2,
            (Value::Integer(i), Value::Float(f)) => i as f64 == f,
            (Value::Float(f1), Value::Float(f2)) => f1 == f2,
            (Value::Float(f), Value::Integer(i)) => i as f64 == f,

            // List comparisons
            (Value::List(l1), Value::List(l2)) => {
                if ptr_eq >= PointerEquality::AllowNested && l1.ptr_eq(&l2) {
                    return Ok(Value::Bool(true));
                }

                if l1.len() != l2.len() {
                    return Ok(Value::Bool(false));
                }

                for (vi1, vi2) in l1.into_iter().zip(l2.into_iter()) {
                    if !generators::check_equality(
                        &co,
                        vi1,
                        vi2,
                        std::cmp::max(ptr_eq, PointerEquality::AllowNested),
                    )
                    .await?
                    {
                        return Ok(Value::Bool(false));
                    }
                }

                true
            }

            (_, Value::List(_)) | (Value::List(_), _) => false,

            // Attribute set comparisons
            (Value::Attrs(a1), Value::Attrs(a2)) => {
                if ptr_eq >= PointerEquality::AllowNested && a1.ptr_eq(&a2) {
                    return Ok(Value::Bool(true));
                }

                // Special-case for derivation comparisons: If both attribute sets
                // have `type = derivation`, compare them by `outPath`.
                match (a1.select("type"), a2.select("type")) {
                    (Some(v1), Some(v2)) => {
                        let s1 = generators::request_force(&co, v1.clone()).await.to_str();
                        let s2 = generators::request_force(&co, v2.clone()).await.to_str();

                        if let (Ok(s1), Ok(s2)) = (s1, s2) {
                            if s1.as_str() == "derivation" && s2.as_str() == "derivation" {
                                // TODO(tazjin): are the outPaths really required,
                                // or should it fall through?
                                let out1 = a1
                                    .select_required("outPath")
                                    .context("comparing derivations")?
                                    .clone();

                                let out2 = a2
                                    .select_required("outPath")
                                    .context("comparing derivations")?
                                    .clone();

                                let result = generators::request_force(&co, out1.clone())
                                    .await
                                    .to_str()?
                                    == generators::request_force(&co, out2.clone())
                                        .await
                                        .to_str()?;
                                return Ok(Value::Bool(result));
                            }
                        }
                    }
                    _ => {}
                };

                if a1.len() != a2.len() {
                    return Ok(Value::Bool(false));
                }

                let iter1 = a1.into_iter_sorted();
                let iter2 = a2.into_iter_sorted();

                for ((k1, v1), (k2, v2)) in iter1.zip(iter2) {
                    if k1 != k2 {
                        return Ok(Value::Bool(false));
                    }

                    if !generators::check_equality(
                        &co,
                        v1,
                        v2,
                        std::cmp::max(ptr_eq, PointerEquality::AllowNested),
                    )
                    .await?
                    {
                        return Ok(Value::Bool(false));
                    }
                }

                true
            }

            (Value::Attrs(_), _) | (_, Value::Attrs(_)) => false,

            (Value::Closure(c1), Value::Closure(c2)) if ptr_eq >= PointerEquality::AllowNested => {
                Rc::ptr_eq(&c1, &c2)
            }

            // Everything else is either incomparable (e.g. internal types) or
            // false.
            _ => false,
        };

        Ok(Value::Bool(result))
    }

    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Integer(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Path(_) => "path",
            Value::Attrs(_) => "set",
            Value::List(_) => "list",
            Value::Closure(_) | Value::Builtin(_) => "lambda",

            // Internal types. Note: These are only elaborated here
            // because it makes debugging easier. If a user ever sees
            // any of these strings, it's a bug.
            Value::Thunk(_) => "internal[thunk]",
            Value::AttrNotFound => "internal[attr_not_found]",
            Value::Blueprint(_) => "internal[blueprint]",
            Value::DeferredUpvalue(_) => "internal[deferred_upvalue]",
            Value::UnresolvedPath(_) => "internal[unresolved_path]",
            Value::Json(_) => "internal[json]",
        }
    }

    gen_cast!(as_bool, bool, "bool", Value::Bool(b), *b);
    gen_cast!(as_int, i64, "int", Value::Integer(x), *x);
    gen_cast!(as_float, f64, "float", Value::Float(x), *x);
    gen_cast!(to_str, NixString, "string", Value::String(s), s.clone());
    gen_cast!(to_attrs, Box<NixAttrs>, "set", Value::Attrs(a), a.clone());
    gen_cast!(to_list, NixList, "list", Value::List(l), l.clone());
    gen_cast!(
        as_closure,
        Rc<Closure>,
        "lambda",
        Value::Closure(c),
        c.clone()
    );

    gen_cast_mut!(as_list_mut, NixList, "list", List);

    gen_is!(is_path, Value::Path(_));
    gen_is!(is_number, Value::Integer(_) | Value::Float(_));
    gen_is!(is_bool, Value::Bool(_));

    /// Compare `self` against other using (fallible) Nix ordering semantics.
    ///
    /// Note that as this returns an `Option<Ordering>` it can not directly be
    /// used as a generator function in the VM. The exact use depends on the
    /// callsite, as the meaning is interpreted in different ways e.g. based on
    /// the comparison operator used.
    ///
    /// The function is intended to be used from within other generator
    /// functions or `gen!` blocks.
    pub async fn nix_cmp_ordering(
        self,
        other: Self,
        co: GenCo,
    ) -> Result<Option<Ordering>, ErrorKind> {
        Self::nix_cmp_ordering_(self, other, co).await
    }

    async fn nix_cmp_ordering_(
        mut myself: Self,
        mut other: Self,
        co: GenCo,
    ) -> Result<Option<Ordering>, ErrorKind> {
        'outer: loop {
            match (myself, other) {
                // same types
                (Value::Integer(i1), Value::Integer(i2)) => return Ok(i1.partial_cmp(&i2)),
                (Value::Float(f1), Value::Float(f2)) => return Ok(f1.partial_cmp(&f2)),
                (Value::String(s1), Value::String(s2)) => return Ok(s1.partial_cmp(&s2)),
                (Value::List(l1), Value::List(l2)) => {
                    for i in 0.. {
                        if i == l2.len() {
                            return Ok(Some(Ordering::Greater));
                        } else if i == l1.len() {
                            return Ok(Some(Ordering::Less));
                        } else if !generators::check_equality(
                            &co,
                            l1[i].clone(),
                            l2[i].clone(),
                            PointerEquality::AllowAll,
                        )
                        .await?
                        {
                            // TODO: do we need to control `top_level` here?
                            myself = generators::request_force(&co, l1[i].clone()).await;
                            other = generators::request_force(&co, l2[i].clone()).await;
                            continue 'outer;
                        }
                    }

                    unreachable!()
                }

                // different types
                (Value::Integer(i1), Value::Float(f2)) => return Ok((i1 as f64).partial_cmp(&f2)),
                (Value::Float(f1), Value::Integer(i2)) => return Ok(f1.partial_cmp(&(i2 as f64))),

                // unsupported types
                (lhs, rhs) => {
                    return Err(ErrorKind::Incomparable {
                        lhs: lhs.type_of(),
                        rhs: rhs.type_of(),
                    })
                }
            }
        }
    }

    pub async fn force(self, co: GenCo) -> Result<Value, ErrorKind> {
        if let Value::Thunk(thunk) = self {
            return thunk.force(co).await;
        }

        Ok(self)
    }

    /// Explain a value in a human-readable way, e.g. by presenting
    /// the docstrings of functions if present.
    pub fn explain(&self) -> String {
        match self {
            Value::Null => "the 'null' value".into(),
            Value::Bool(b) => format!("the boolean value '{}'", b),
            Value::Integer(i) => format!("the integer '{}'", i),
            Value::Float(f) => format!("the float '{}'", f),
            Value::String(s) => format!("the string '{}'", s),
            Value::Path(p) => format!("the path '{}'", p.to_string_lossy()),
            Value::Attrs(attrs) => format!("a {}-item attribute set", attrs.len()),
            Value::List(list) => format!("a {}-item list", list.len()),

            Value::Closure(f) => {
                if let Some(name) = &f.lambda.name {
                    format!("the user-defined Nix function '{}'", name)
                } else {
                    "a user-defined Nix function".to_string()
                }
            }

            Value::Builtin(b) => {
                let mut out = format!("the builtin function '{}'", b.name());
                if let Some(docs) = b.documentation() {
                    out.push_str("\n\n");
                    out.push_str(docs);
                }
                out
            }

            // TODO: handle suspended thunks with a different explanation instead of panicking
            Value::Thunk(t) => t.value().explain(),

            Value::AttrNotFound
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_)
            | Value::UnresolvedPath(_)
            | Value::Json(_) => "an internal Tvix evaluator value".into(),
        }
    }
}

trait TotalDisplay {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result;
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.total_fmt(f, &mut Default::default())
    }
}

/// Emulates the C++-Nix style formatting of floats, which diverges
/// significantly from Rust's native float formatting.
fn total_fmt_float<F: std::fmt::Write>(num: f64, mut f: F) -> std::fmt::Result {
    let mut buf = [b'0'; lexical_core::BUFFER_SIZE];
    let mut s = lexical_core::write_with_options::<f64, { CXX_LITERAL }>(
        num,
        &mut buf,
        &WRITE_FLOAT_OPTIONS,
    );

    // apply some postprocessing on the buffer. If scientific
    // notation is used (we see an `e`), and the next character is
    // a digit, add the missing `+` sign.)
    let mut new_s = Vec::with_capacity(s.len());

    if s.contains(&b'e') {
        for (i, c) in s.iter().enumerate() {
            // encountered `e`
            if c == &b'e' {
                // next character is a digit (so no negative exponent)
                if s.len() > i && s[i + 1].is_ascii_digit() {
                    // copy everything from the start up to (including) the e
                    new_s.extend_from_slice(&s[0..=i]);
                    // add the missing '+'
                    new_s.push(b'+');
                    // check for the remaining characters.
                    // If it's only one, we need to prepend a trailing zero
                    if s.len() == i + 2 {
                        new_s.push(b'0');
                    }
                    new_s.extend_from_slice(&s[i + 1..]);
                    break;
                }
            }
        }

        // if we modified the scientific notation, flip the reference
        if !new_s.is_empty() {
            s = &mut new_s
        }
    }
    // else, if this is not scientific notation, and there's a
    // decimal point, make sure we really drop trailing zeroes.
    // In some cases, lexical_core doesn't.
    else if s.contains(&b'.') {
        for (i, c) in s.iter().enumerate() {
            // at `.``
            if c == &b'.' {
                // trim zeroes from the right side.
                let frac = String::from_utf8_lossy(&s[i + 1..]);
                let frac_no_trailing_zeroes = frac.trim_end_matches('0');

                if frac.len() != frac_no_trailing_zeroes.len() {
                    // we managed to strip something, construct new_s
                    if frac_no_trailing_zeroes.is_empty() {
                        // if frac_no_trailing_zeroes is empty, the fractional part was all zeroes, so we can drop the decimal point as well
                        new_s.extend_from_slice(&s[0..=i - 1]);
                    } else {
                        // else, assemble the rest of the string
                        new_s.extend_from_slice(&s[0..=i]);
                        new_s.extend_from_slice(frac_no_trailing_zeroes.as_bytes());
                    }

                    // flip the reference
                    s = &mut new_s;
                    break;
                }
            }
        }
    }

    write!(f, "{}", format!("{}", String::from_utf8_lossy(s)))
}

impl TotalDisplay for Value {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(true) => f.write_str("true"),
            Value::Bool(false) => f.write_str("false"),
            Value::Integer(num) => write!(f, "{}", num),
            Value::String(s) => s.fmt(f),
            Value::Path(p) => p.display().fmt(f),
            Value::Attrs(attrs) => attrs.total_fmt(f, set),
            Value::List(list) => list.total_fmt(f, set),
            Value::Closure(_) => f.write_str("lambda"), // TODO: print position
            Value::Builtin(builtin) => builtin.fmt(f),

            // Nix prints floats with a maximum precision of 5 digits
            // only. Except when it decides to use scientific notation
            // (with a + after the `e`, and zero-padded to 0 digits)
            Value::Float(num) => total_fmt_float(*num, f),

            // internal types
            Value::AttrNotFound => f.write_str("internal[not found]"),
            Value::Blueprint(_) => f.write_str("internal[blueprint]"),
            Value::DeferredUpvalue(_) => f.write_str("internal[deferred_upvalue]"),
            Value::UnresolvedPath(_) => f.write_str("internal[unresolved_path]"),
            Value::Json(_) => f.write_str("internal[json]"),

            // Delegate thunk display to the type, as it must handle
            // the case of already evaluated or cyclic thunks.
            Value::Thunk(t) => t.total_fmt(f, set),
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<f64> for Value {
    fn from(i: f64) -> Self {
        Self::Float(i)
    }
}

impl From<PathBuf> for Value {
    fn from(path: PathBuf) -> Self {
        Self::Path(Box::new(path))
    }
}

fn type_error(expected: &'static str, actual: &Value) -> ErrorKind {
    ErrorKind::TypeError {
        expected,
        actual: actual.type_of(),
    }
}

#[cfg(test)]
mod tests {
    mod floats {
        use crate::value::total_fmt_float;

        #[test]
        fn format_float() {
            let ff = vec![
                (0f64, "0"),
                (1.0f64, "1"),
                (-0.01, "-0.01"),
                (5e+22, "5e+22"),
                (1e6, "1e+06"),
                (-2E-2, "-0.02"),
                (6.626e-34, "6.626e-34"),
                (9_224_617.445_991_227, "9.22462e+06"),
            ];
            for (n, expected) in ff.iter() {
                let mut buf = String::new();
                let res = total_fmt_float(*n, &mut buf);
                assert!(res.is_ok());
                assert_eq!(
                    expected, &buf,
                    "{} should be formatted as {}, but got {}",
                    n, expected, &buf
                );
            }
        }
    }
}
