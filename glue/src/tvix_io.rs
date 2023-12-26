//! This module implements a wrapper around tvix-eval's [EvalIO] type,
//! adding functionality which is required by tvix-cli:
//!
//! 1. Marking plain paths known to the reference scanner.
//! 2. Handling the C++ Nix `__corepkgs__`-hack for nixpkgs bootstrapping.
//!
//! All uses of [EvalIO] in tvix-cli must make use of this wrapper,
//! otherwise fundamental features like nixpkgs bootstrapping and hash
//! calculation will not work.

use std::io;
use std::path::{Path, PathBuf};
use tvix_eval::{EvalIO, FileType};

// TODO: Merge this together with TvixStoreIO?
pub struct TvixIO<T: EvalIO> {
    // Actual underlying [EvalIO] implementation.
    actual: T,
}

impl<T: EvalIO> TvixIO<T> {
    pub fn new(actual: T) -> Self {
        Self { actual }
    }
}

impl<T: EvalIO> EvalIO for TvixIO<T> {
    fn store_dir(&self) -> Option<String> {
        self.actual.store_dir()
    }

    fn import_path(&self, path: &Path) -> io::Result<PathBuf> {
        let imported_path = self.actual.import_path(path)?;
        Ok(imported_path)
    }

    fn path_exists(&self, path: &Path) -> io::Result<bool> {
        if path.starts_with("/__corepkgs__") {
            return Ok(true);
        }

        self.actual.path_exists(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        // Bundled version of corepkgs/fetchurl.nix. The counterpart
        // of this happens in [crate::configure_nix_path], where the `nix_path`
        // of the evaluation has `nix=/__corepkgs__` added to it.
        //
        // This workaround is similar to what cppnix does for passing
        // the path through.
        //
        // TODO: this comparison is bad and allocates, we should use
        // the sane path library.
        if path.starts_with("/__corepkgs__/fetchurl.nix") {
            return Ok(include_str!("fetchurl.nix").to_string());
        }

        self.actual.read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<(bytes::Bytes, FileType)>> {
        self.actual.read_dir(path)
    }
}
