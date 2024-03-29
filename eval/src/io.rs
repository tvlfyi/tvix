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

use std::{
    io,
    path::{Path, PathBuf},
};

#[cfg(target_family = "unix")]
use std::os::unix::ffi::OsStringExt;

/// Types of files as represented by `builtins.readDir` in Nix.
#[derive(Debug)]
pub enum FileType {
    Directory,
    Regular,
    Symlink,
    Unknown,
}

/// Represents all possible filesystem interactions that exist in the Nix
/// language, and that need to be executed somehow.
///
/// This trait is specifically *only* concerned with what is visible on the
/// level of the language. All internal implementation details are not part of
/// this trait.
pub trait EvalIO {
    /// Verify whether the file at the specified path exists.
    ///
    /// This is used for the following language evaluation cases:
    ///
    /// * checking whether a file added to the `NIX_PATH` actually exists when
    ///   it is referenced in `<...>` brackets.
    /// * `builtins.pathExists :: path -> bool`
    fn path_exists(&self, path: &Path) -> io::Result<bool>;

    /// Read the file at the specified path to a `Vec<u8>`.
    ///
    /// This is used for the following language evaluation cases:
    ///
    /// * `builtins.readFile :: path -> string`
    /// * `builtins.import :: path -> any`
    fn read_to_end(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Read the directory at the specified path and return the names
    /// of its entries associated with their [`FileType`].
    ///
    /// This is used for the following language evaluation cases:
    ///
    /// * `builtins.readDir :: path -> attrs<filename, filetype>`
    fn read_dir(&self, path: &Path) -> io::Result<Vec<(bytes::Bytes, FileType)>>;

    /// Import the given path. What this means depends on the implementation,
    /// for example for a `std::io`-based implementation this might be a no-op,
    /// while for a Tvix store this might be a copy of the given files to the
    /// store.
    ///
    /// This is used for the following language evaluation cases:
    ///
    /// * string coercion of path literals (e.g. `/foo/bar`), which are expected
    ///   to return a path
    /// * `builtins.toJSON` on a path literal, also expected to return a path
    fn import_path(&self, path: &Path) -> io::Result<PathBuf>;

    /// Returns the root of the store directory, if such a thing
    /// exists in the evaluation context.
    ///
    /// This is used for the following language evaluation cases:
    ///
    /// * `builtins.storeDir :: string`
    fn store_dir(&self) -> Option<String> {
        None
    }
}

/// Implementation of [`EvalIO`] that simply uses the equivalent
/// standard library functions, i.e. does local file-IO.
#[cfg(feature = "impure")]
pub struct StdIO;

// TODO: we might want to make this whole impl to be target_family = "unix".
#[cfg(feature = "impure")]
impl EvalIO for StdIO {
    fn path_exists(&self, path: &Path) -> io::Result<bool> {
        path.try_exists()
    }

    fn read_to_end(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<(bytes::Bytes, FileType)>> {
        let mut result = vec![];

        for entry in path.read_dir()? {
            let entry = entry?;
            let file_type = entry.metadata()?.file_type();

            let val = if file_type.is_dir() {
                FileType::Directory
            } else if file_type.is_file() {
                FileType::Regular
            } else if file_type.is_symlink() {
                FileType::Symlink
            } else {
                FileType::Unknown
            };

            result.push((entry.file_name().into_vec().into(), val))
        }

        Ok(result)
    }

    // this is a no-op for `std::io`, as the user can already refer to
    // the path directly
    fn import_path(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}

/// Dummy implementation of [`EvalIO`], can be used in contexts where
/// IO is not available but code should "pretend" that it is.
pub struct DummyIO;

impl EvalIO for DummyIO {
    fn path_exists(&self, _: &Path) -> io::Result<bool> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "I/O methods are not implemented in DummyIO",
        ))
    }

    fn read_to_end(&self, _: &Path) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "I/O methods are not implemented in DummyIO",
        ))
    }

    fn read_dir(&self, _: &Path) -> io::Result<Vec<(bytes::Bytes, FileType)>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "I/O methods are not implemented in DummyIO",
        ))
    }

    fn import_path(&self, _: &Path) -> io::Result<PathBuf> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "I/O methods are not implemented in DummyIO",
        ))
    }
}
