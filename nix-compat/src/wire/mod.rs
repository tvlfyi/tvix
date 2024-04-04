//! Module parsing and emitting the wire format used by Nix, both in the
//! nix-daemon protocol as well as in the NAR format.

#[cfg(feature = "async")]
pub mod bytes;

#[cfg(feature = "async")]
mod bytes_writer;
#[cfg(feature = "async")]
pub use bytes_writer::BytesWriter;

#[cfg(feature = "async")]
pub mod primitive;

#[cfg(feature = "async")]
pub mod worker_protocol;
