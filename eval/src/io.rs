//! Interface for injecting I/O-related functionality into tvix-eval.
//!
//! The Nix language contains several builtins (e.g. `builtins.readDir`), as
//! well as language feature (e.g. string-"coercion" of paths) that interact
//! with the filesystem.
//!
//! The language evaluator implemented by this crate does not depend on any
//! particular filesystem interaction model. Instead, this module provides a
//! trait that can be implemented by tvix-eval callers to provide the
//! functionality they desire.
//!
//! In theory this can be used to implement "mocked" filesystem interactions, or
//! interaction with remote filesystems, etc.
//!
//! In the context of Nix builds, callers also use this interface to determine
//! how store paths are opened and so on.

use crate::errors::ErrorKind;
use smol_str::SmolStr;
use std::path::{Path, PathBuf};

#[cfg(feature = "impure")]
use std::rc::Rc;

/// Types of files as represented by `builtins.readDir` in Nix.
#[derive(Debug)]
pub enum FileType {
    Directory,
    Regular,
    Symlink,
    Unknown,
}

/// Defines how filesystem interaction occurs inside of tvix-eval.
pub trait EvalIO {
    /// Verify whether the file at the specified path exists.
    fn path_exists(&self, path: PathBuf) -> Result<bool, ErrorKind>;

    /// Read the file at the specified path to a string.
    fn read_to_string(&self, path: PathBuf) -> Result<String, ErrorKind>;

    /// Read the directory at the specified path and return the names
    /// of its entries associated with their [`FileType`].
    fn read_dir(&self, path: PathBuf) -> Result<Vec<(SmolStr, FileType)>, ErrorKind>;

    /// Import the given path. What this means depends on the
    /// implementation, for example for a `std::io`-based
    /// implementation this might be a no-op, while for a Tvix store
    /// this might be a copy of the given files to the store.
    ///
    /// This is primarily used in the context of things like coercing
    /// a local path to a string, or builtins like `path`.
    fn import_path(&self, path: &Path) -> Result<PathBuf, ErrorKind>;

    /// Returns the root of the store directory, if such a thing
    /// exists in the evaluation context.
    fn store_dir(&self) -> Option<String> {
        None
    }
}

/// Implementation of [`EvalIO`] that simply uses the equivalent
/// standard library functions, i.e. does local file-IO.
#[cfg(feature = "impure")]
pub struct StdIO;

#[cfg(feature = "impure")]
impl EvalIO for StdIO {
    fn path_exists(&self, path: PathBuf) -> Result<bool, ErrorKind> {
        path.try_exists().map_err(|e| ErrorKind::IO {
            path: Some(path),
            error: Rc::new(e),
        })
    }

    fn read_to_string(&self, path: PathBuf) -> Result<String, ErrorKind> {
        std::fs::read_to_string(&path).map_err(|e| ErrorKind::IO {
            path: Some(path),
            error: Rc::new(e),
        })
    }

    fn read_dir(&self, path: PathBuf) -> Result<Vec<(SmolStr, FileType)>, ErrorKind> {
        let mut result = vec![];

        let mk_err = |err| ErrorKind::IO {
            path: Some(path.clone()),
            error: Rc::new(err),
        };

        for entry in path.read_dir().map_err(mk_err)? {
            let entry = entry.map_err(mk_err)?;
            let file_type = entry
                .metadata()
                .map_err(|err| ErrorKind::IO {
                    path: Some(entry.path()),
                    error: Rc::new(err),
                })?
                .file_type();

            let val = if file_type.is_dir() {
                FileType::Directory
            } else if file_type.is_file() {
                FileType::Regular
            } else if file_type.is_symlink() {
                FileType::Symlink
            } else {
                FileType::Unknown
            };

            result.push((SmolStr::new(entry.file_name().to_string_lossy()), val));
        }

        Ok(result)
    }

    // this is a no-op for `std::io`, as the user can already refer to
    // the path directly
    fn import_path(&self, path: &Path) -> Result<PathBuf, ErrorKind> {
        Ok(path.to_path_buf())
    }
}

/// Dummy implementation of [`EvalIO`], can be used in contexts where
/// IO is not available but code should "pretend" that it is.
pub struct DummyIO;

impl EvalIO for DummyIO {
    fn path_exists(&self, _: PathBuf) -> Result<bool, ErrorKind> {
        Err(ErrorKind::NotImplemented(
            "I/O methods are not implemented in DummyIO",
        ))
    }

    fn read_to_string(&self, _: PathBuf) -> Result<String, ErrorKind> {
        Err(ErrorKind::NotImplemented(
            "I/O methods are not implemented in DummyIO",
        ))
    }

    fn read_dir(&self, _: PathBuf) -> Result<Vec<(SmolStr, FileType)>, ErrorKind> {
        Err(ErrorKind::NotImplemented(
            "I/O methods are not implemented in DummyIO",
        ))
    }

    fn import_path(&self, _: &Path) -> Result<PathBuf, ErrorKind> {
        Err(ErrorKind::NotImplemented(
            "I/O methods are not implemented in DummyIO",
        ))
    }
}
