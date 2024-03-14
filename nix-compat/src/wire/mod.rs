//! Module parsing and emitting the wire format used by Nix, both in the
//! nix-daemon protocol as well as in the NAR format.

#[cfg(feature = "async")]
pub mod bytes;

#[cfg(feature = "async")]
pub mod primitive;
