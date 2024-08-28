//! # Using derive
//!
//! 1. [Overview](#overview)
//! 3. [Attributes](#attributes)
//!     1. [Container attributes](#container-attributes)
//!         1. [`#[nix(from_str)]`](#nixfrom_str)
//!         2. [`#[nix(from = "FromType")]`](#nixfrom--fromtype)
//!         3. [`#[nix(try_from = "FromType")]`](#nixtry_from--fromtype)
//!         4. [`#[nix(crate = "...")]`](#nixcrate--)
//!     2. [Variant attributes](#variant-attributes)
//!         1. [`#[nix(version = "range")]`](#nixversion--range)
//!     3. [Field attributes](#field-attributes)
//!         1. [`#[nix(version = "range")]`](#nixversion--range-1)
//!         2. [`#[nix(default)]`](#nixdefault)
//!         3. [`#[nix(default = "path")]`](#nixdefault--path)
//!
//! ## Overview
//!
//! This crate contains derive macros and function-like macros for implementing
//! `NixDeserialize` with less boilerplate.
//!
//! ### Examples
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! struct Unnamed(u64, String);
//! ```
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! struct Fields {
//!     number: u64,
//!     message: String,
//! };
//! ```
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! struct Ignored;
//! ```
//!
//! ## Attributes
//!
//! To customize the derived trait implementations you can add
//! [attributes](https://doc.rust-lang.org/reference/attributes.html)
//! to containers, fields and variants.
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! #[nix(crate="nix_compat")] // <-- This is a container attribute
//! struct Fields {
//!     number: u64,
//!     #[nix(version="..20")] // <-- This is a field attribute
//!     message: String,
//! };
//!
//! #[derive(NixDeserialize)]
//! #[nix(crate="nix_compat")] // <-- This is also a container attribute
//! enum E {
//!     #[nix(version="..=9")] // <-- This is a variant attribute
//!     A(u64),
//!     #[nix(version="10..")] // <-- This is also a variant attribute
//!     B(String),
//! }
//! ```
//!
//! ### Container attributes
//!
//! ##### `#[nix(from_str)]`
//!
//! When `from_str` is specified the fields are all ignored and instead a
//! `String` is first deserialized and then `FromStr::from_str` is used
//! to convert this `String` to the container type.
//!
//! This means that the container must implement `FromStr` and the error
//! returned from the `from_str` must implement `Display`.
//!
//! ###### Example
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! #[nix(from_str)]
//! struct MyString(String);
//! impl std::str::FromStr for MyString {
//!     type Err = String;
//!     fn from_str(s: &str) -> Result<Self, Self::Err> {
//!         if s != "bad string" {
//!             Ok(MyString(s.to_string()))
//!         } else {
//!             Err("Got a bad string".to_string())
//!         }
//!     }
//! }
//! ```
//!
//! ##### `#[nix(from = "FromType")]`
//!
//! When `from` is specified the fields are all ignored and instead a
//! value of `FromType` is first deserialized and then `From::from` is
//! used to convert from this value to the container type.
//!
//! This means that the container must implement `From<FromType>` and
//! `FromType` must implement `NixDeserialize`.
//!
//! ###### Example
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! #[nix(from="usize")]
//! struct MyValue(usize);
//! impl From<usize> for MyValue {
//!     fn from(val: usize) -> Self {
//!         MyValue(val)
//!     }
//! }
//! ```
//!
//! ##### `#[nix(try_from = "FromType")]`
//!
//! With `try_from` a value of `FromType` is first deserialized and then
//! `TryFrom::try_from` is used to convert from this value to the container
//! type.
//!
//! This means that the container must implement `TryFrom<FromType>` and
//! `FromType` must implement `NixDeserialize`.
//! The error returned from `try_from` also needs to implement `Display`.
//!
//! ###### Example
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! #[nix(try_from="usize")]
//! struct WrongAnswer(usize);
//! impl TryFrom<usize> for WrongAnswer {
//!     type Error = String;
//!     fn try_from(val: usize) -> Result<Self, Self::Error> {
//!         if val != 42 {
//!             Ok(WrongAnswer(val))
//!         } else {
//!             Err("Got the answer to life the universe and everything".to_string())
//!         }
//!     }
//! }
//! ```
//!
//! ##### `#[nix(crate = "...")]`
//!
//! Specify the path to the `nix-compat` crate instance to use when referring
//! to the API in the generated code. This is usually not needed.
//!
//! ### Variant attributes
//!
//! ##### `#[nix(version = "range")]`
//!
//! Specifies the protocol version range where this variant is used.
//! When deriving an enum the `version` attribute is used to select which
//! variant of the enum to deserialize. The range is for minor version and
//! the version ranges of all variants combined must cover all versions
//! without any overlap or the first variant that matches is selected.
//!
//! ###### Example
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #[derive(NixDeserialize)]
//! enum Testing {
//!     #[nix(version="..=18")]
//!     OldVersion(u64),
//!     #[nix(version="19..")]
//!     NewVersion(String),
//! }
//! ```
//!
//! ### Field attributes
//!
//! ##### `#[nix(version = "range")]`
//!
//! Specifies the protocol version range where this field is included.
//! The range is for minor version. For example `version = "..20"`
//! includes the field in protocol versions `1.0` to `1.19` and skips
//! it in version `1.20` and above.
//!
//! ###### Example
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! struct Field {
//!     number: u64,
//!     #[nix(version="..20")]
//!     messsage: String,
//! }
//! ```
//!
//! ##### `#[nix(default)]`
//!
//! When a field is skipped because the active protocol version falls
//! outside the range specified in [`#[nix(version = "range")]`](#nixversion--range-1)
//! this attribute indicates that `Default::default()` should be used
//! to get a value for the field. This is also the default
//! when you only specify [`#[nix(version = "range")]`](#nixversion--range-1).
//!
//! ###### Example
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! struct Field {
//!     number: u64,
//!     #[nix(version="..20", default)]
//!     messsage: String,
//! }
//! ```
//!
//! ##### `#[nix(default = "path")]`
//!
//! When a field is skipped because the active protocol version falls
//! outside the range specified in [`#[nix(version = "range")]`](#nixversion--range-1)
//! this attribute indicates that the function in `path` should be called to
//! get a default value for the field. The given function must be callable
//! as `fn() -> T`.
//! For example `default = "my_value"` would call `my_value()` and `default =
//! "AType::empty"` would call `AType::empty()`.
//!
//! ###### Example
//!
//! ```rust
//! # use nix_compat_derive::NixDeserialize;
//! #
//! #[derive(NixDeserialize)]
//! struct Field {
//!     number: u64,
//!     #[nix(version="..20", default="missing_string")]
//!     messsage: String,
//! }
//!
//! fn missing_string() -> String {
//!     "missing string".to_string()
//! }
//! ```

use internal::inputs::RemoteInput;
use proc_macro::TokenStream;
use syn::{parse_quote, DeriveInput};

mod de;
mod internal;

#[proc_macro_derive(NixDeserialize, attributes(nix))]
pub fn derive_nix_deserialize(item: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(item as DeriveInput);
    let nnixrs: syn::Path = parse_quote!(::nix_compat);
    de::expand_nix_deserialize(nnixrs, &mut input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Macro to implement `NixDeserialize` on a type.
/// Sometimes you can't use the deriver to implement `NixDeserialize`
/// (like when dealing with types in Rust standard library) but don't want
/// to implement it yourself. So this macro can be used for those situations
/// where you would derive using `#[nix(from_str)]`,
/// `#[nix(from = "FromType")]` or `#[nix(try_from = "FromType")]` if you
/// could.
///
/// #### Example
///
/// ```rust
/// # use nix_compat_derive::nix_deserialize_remote;
/// #
/// struct MyU64(u64);
///
/// impl From<u64> for MyU64 {
///     fn from(value: u64) -> Self {
///         Self(value)
///     }
/// }
///
/// nix_deserialize_remote!(#[nix(from="u64")] MyU64);
/// ```
#[proc_macro]
pub fn nix_deserialize_remote(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as RemoteInput);
    let crate_path = parse_quote!(::nix_compat);
    de::expand_nix_deserialize_remote(crate_path, &input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
