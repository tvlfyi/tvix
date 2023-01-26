//! This module implements (temporary) compatibility shims between
//! Tvix and C++ Nix.
//!
//! These are not intended to be long-lived, but should bootstrap Tvix
//! by piggybacking off functionality that already exists in Nix and
//! is still being implemented in Tvix.

use std::cell::RefCell;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::{io, path::PathBuf};

use crate::known_paths::KnownPaths;
use smol_str::SmolStr;
use tvix_eval::{ErrorKind, EvalIO, FileType, StdIO};

/// Compatibility implementation of [`EvalIO`] that uses C++ Nix to
/// write files to the Nix store.
pub struct NixCompatIO {
    /// Most IO requests are tunneled through to [`tvix_eval::StdIO`]
    /// instead.
    underlying: StdIO,

    /// Ingested paths must be reported to this known paths tracker
    /// for accurate build reference scanning.
    known_paths: Rc<RefCell<KnownPaths>>,
}

impl EvalIO for NixCompatIO {
    fn store_dir(&self) -> Option<String> {
        Some("/nix/store".into())
    }

    // Pass path imports through to `nix-store --add`
    fn import_path(&self, path: &Path) -> Result<PathBuf, ErrorKind> {
        self.add_to_store(path).map_err(|error| ErrorKind::IO {
            error: std::rc::Rc::new(error),
            path: Some(path.to_path_buf()),
        })
    }

    // Pass the rest of the functions through to `Self::underlying`
    fn path_exists(&self, path: PathBuf) -> Result<bool, ErrorKind> {
        self.underlying.path_exists(path)
    }

    fn read_to_string(&self, path: PathBuf) -> Result<String, ErrorKind> {
        self.underlying.read_to_string(path)
    }

    fn read_dir(&self, path: PathBuf) -> Result<Vec<(SmolStr, FileType)>, ErrorKind> {
        self.underlying.read_dir(path)
    }
}

impl NixCompatIO {
    pub fn new(known_paths: Rc<RefCell<KnownPaths>>) -> Self {
        NixCompatIO {
            underlying: StdIO,
            known_paths,
        }
    }

    /// Add a path to the Nix store using the `nix-store --add`
    /// functionality from C++ Nix.
    fn add_to_store(&self, path: &Path) -> Result<PathBuf, io::Error> {
        if !path.try_exists()? {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }

        let mut cmd = Command::new("nix-store");
        cmd.arg("--add");
        cmd.arg(path);

        let out = cmd.output()?;

        if !out.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            ));
        }

        let out_path_str = String::from_utf8(out.stdout)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        let out_path_trimmed = out_path_str.trim();

        self.known_paths.borrow_mut().plain(out_path_trimmed);

        let mut out_path = PathBuf::new();
        out_path.push(out_path_trimmed);
        Ok(out_path)
    }
}
