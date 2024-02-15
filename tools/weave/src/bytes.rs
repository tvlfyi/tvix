use owning_ref::{OwningRef, StableAddress};
use polars::export::arrow::buffer::Buffer;
use std::ops::Deref;

/// An shared `[[u8; N]]` backed by a Polars [Buffer].
pub type FixedBytes<const N: usize> = OwningRef<Bytes, [[u8; N]]>;

/// Wrapper struct to make [Buffer] implement [StableAddress].
/// TODO(edef): upstream the `impl`
pub struct Bytes(pub Buffer<u8>);

/// SAFETY: [Buffer] is always an Arc+Vec indirection.
unsafe impl StableAddress for Bytes {}

impl Bytes {
    pub fn map<U: ?Sized>(self, f: impl FnOnce(&[u8]) -> &U) -> OwningRef<Self, U> {
        OwningRef::new(self).map(f)
    }
}

impl Deref for Bytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}
