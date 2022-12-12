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

use std::path::PathBuf;

use crate::errors::ErrorKind;

/// Defines how filesystem interaction occurs inside of tvix-eval.
pub trait EvalIO {
    /// Verify whether the file at the specified path exists.
    fn path_exists(&self, path: PathBuf) -> Result<bool, ErrorKind>;

    /// Read the file at the specified path to a string.
    fn read_to_string(&self, path: PathBuf) -> Result<String, ErrorKind>;
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
            error: std::rc::Rc::new(e),
        })
    }

    fn read_to_string(&self, path: PathBuf) -> Result<String, ErrorKind> {
        std::fs::read_to_string(&path).map_err(|e| ErrorKind::IO {
            path: Some(path),
            error: std::rc::Rc::new(e),
        })
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
}
