//! This module implements (temporary) compatibility shims between
//! Tvix and C++ Nix.
//!
//! These are not intended to be long-lived, but should bootstrap Tvix
//! by piggybacking off functionality that already exists in Nix and
//! is still being implemented in Tvix.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::RwLock;
use std::{io, path::PathBuf};

use smol_str::SmolStr;
use tvix_eval::{EvalIO, FileType, StdIO};

/// Compatibility implementation of [`EvalIO`] that uses C++ Nix to
/// write files to the Nix store.
pub struct NixCompatIO {
    /// Most IO requests are tunneled through to [`tvix_eval::StdIO`]
    /// instead.
    underlying: StdIO,

    /// Cache paths for identical files being imported to the store.
    // TODO(tazjin): This could be done better by having a thunk cache
    // for these calls on the eval side, but that is a little more
    // complex.
    import_cache: RwLock<HashMap<PathBuf, PathBuf>>,
}

impl EvalIO for NixCompatIO {
    fn store_dir(&self) -> Option<String> {
        Some("/nix/store".into())
    }

    // Pass path imports through to `nix-store --add`
    fn import_path(&self, path: &Path) -> Result<PathBuf, io::Error> {
        let path = path.to_owned();
        if let Some(path) = self
            .import_cache
            .read()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
            .get(&path)
        {
            return Ok(path.to_path_buf());
        }

        let store_path = self.add_to_store(&path)?;

        self.import_cache
            .write()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?
            .insert(path, store_path.clone());

        Ok(store_path)
    }

    // Pass the rest of the functions through to `Self::underlying`
    fn path_exists(&self, path: &Path) -> Result<bool, io::Error> {
        if path.starts_with("/__corepkgs__") {
            return Ok(true);
        }

        self.underlying.path_exists(path)
    }

    fn read_to_string(&self, path: &Path) -> Result<String, io::Error> {
        // Bundled version of corepkgs/fetchurl.nix. This workaround
        // is similar to what cppnix does for passing the path
        // through.
        //
        // TODO: this comparison is bad and allocates, we should use
        // the sane path library.
        if path.starts_with("/__corepkgs__/fetchurl.nix") {
            return Ok(include_str!("fetchurl.nix").to_string());
        }

        self.underlying.read_to_string(path)
    }

    fn read_dir(&self, path: &Path) -> Result<Vec<(SmolStr, FileType)>, io::Error> {
        self.underlying.read_dir(path)
    }
}

impl NixCompatIO {
    pub fn new() -> Self {
        NixCompatIO {
            underlying: StdIO,
            import_cache: RwLock::new(HashMap::new()),
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

        let mut out_path = PathBuf::new();
        out_path.push(out_path_trimmed);
        Ok(out_path)
    }
}
