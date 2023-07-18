//! This module implements a wrapper around tvix-eval's [EvalIO] type,
//! adding functionality which is required by tvix-cli:
//!
//! 1. Marking plain paths known to the reference scanner.
//! 2. Handling the C++ Nix `__corepkgs__`-hack for nixpkgs bootstrapping.
//!
//! All uses of [EvalIO] in tvix-cli must make use of this wrapper,
//! otherwise fundamental features like nixpkgs bootstrapping and hash
//! calculation will not work.

use crate::KnownPaths;
use std::cell::RefCell;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use tvix_eval::{EvalIO, FileType};

pub(crate) struct TvixIO<T: EvalIO> {
    /// Ingested paths must be reported to this known paths tracker
    /// for accurate build reference scanning.
    known_paths: Rc<RefCell<KnownPaths>>,

    // Actual underlying [EvalIO] implementation.
    actual: T,
}

impl<T: EvalIO> TvixIO<T> {
    pub(crate) fn new(known_paths: Rc<RefCell<KnownPaths>>, actual: T) -> Self {
        Self {
            known_paths,
            actual,
        }
    }
}

impl<T: EvalIO> EvalIO for TvixIO<T> {
    fn store_dir(&self) -> Option<String> {
        self.actual.store_dir()
    }

    fn import_path(&self, path: &Path) -> Result<PathBuf, io::Error> {
        let imported_path = self.actual.import_path(path)?;
        self.known_paths
            .borrow_mut()
            .plain(imported_path.to_string_lossy());

        Ok(imported_path)
    }

    fn path_exists(&self, path: &Path) -> Result<bool, io::Error> {
        if path.starts_with("/__corepkgs__") {
            return Ok(true);
        }

        self.actual.path_exists(path)
    }

    fn read_to_string(&self, path: &Path) -> Result<String, io::Error> {
        // Bundled version of corepkgs/fetchurl.nix. The counterpart
        // of this happens in `main`, where the `nix_path` of the
        // evaluation has `nix=/__corepkgs__` added to it.
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

    fn read_dir(&self, path: &Path) -> Result<Vec<(Vec<u8>, FileType)>, io::Error> {
        self.actual.read_dir(path)
    }
}
