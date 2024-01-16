use path_clean::PathClean;
use std::convert::Infallible;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::errors::{CatchableErrorKind, ErrorKind};
use crate::EvalIO;

#[derive(Debug, Clone, PartialEq, Eq)]
enum NixSearchPathEntry {
    /// Resolve subdirectories of this path within `<...>` brackets. This
    /// corresponds to bare paths within the `NIX_PATH` environment variable
    ///
    /// For example, with `NixSearchPathEntry::Path("/example")` and the following
    /// directory structure:
    ///
    /// ```notrust
    /// example
    /// └── subdir
    ///     └── grandchild
    /// ```
    ///
    /// A Nix path literal `<subdir>` would resolve to `/example/subdir`, and a
    /// Nix path literal `<subdir/grandchild>` would resolve to
    /// `/example/subdir/grandchild`
    Path(PathBuf),

    /// Resolve paths starting with `prefix` as subdirectories of `path`. This
    /// corresponds to `prefix=path` within the `NIX_PATH` environment variable.
    ///
    /// For example, with `NixSearchPathEntry::Prefix { prefix: "prefix", path:
    /// "/example" }` and the following directory structure:
    ///
    /// ```notrust
    /// example
    /// └── subdir
    ///     └── grandchild
    /// ```
    ///
    /// A Nix path literal `<prefix/subdir>` would resolve to `/example/subdir`,
    /// and a Nix path literal `<prefix/subdir/grandchild>` would resolve to
    /// `/example/subdir/grandchild`
    Prefix { prefix: PathBuf, path: PathBuf },
}

fn canonicalise(path: PathBuf) -> Result<PathBuf, ErrorKind> {
    let absolute = if path.is_absolute() {
        path
    } else {
        // TODO(tazjin): probably panics in wasm?
        std::env::current_dir()
            .map_err(|e| ErrorKind::IO {
                path: Some(path.clone()),
                error: e.into(),
            })?
            .join(path)
    }
    .clean();

    Ok(absolute)
}

impl NixSearchPathEntry {
    /// Determine whether this path entry matches the given lookup path.
    ///
    /// For bare paths, an entry is considered to match if a matching
    /// file exists under it.
    ///
    /// For prefixed path, an entry matches if the prefix does.
    // TODO(tazjin): verify these rules in the C++ impl, seems fishy.
    fn resolve<IO>(&self, io: IO, lookup_path: &Path) -> Result<Option<PathBuf>, ErrorKind>
    where
        IO: AsRef<dyn EvalIO>,
    {
        let path = match self {
            NixSearchPathEntry::Path(parent) => canonicalise(parent.join(lookup_path))?,

            NixSearchPathEntry::Prefix { prefix, path } => {
                if let Ok(child_path) = lookup_path.strip_prefix(prefix) {
                    canonicalise(path.join(child_path))?
                } else {
                    return Ok(None);
                }
            }
        };

        if io.as_ref().path_exists(&path).map_err(|e| ErrorKind::IO {
            path: Some(path.clone()),
            error: e.into(),
        })? {
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }
}

impl FromStr for NixSearchPathEntry {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('=') {
            Some((prefix, path)) => Ok(Self::Prefix {
                prefix: prefix.into(),
                path: path.into(),
            }),
            None => Ok(Self::Path(s.into())),
        }
    }
}

/// Struct implementing the format and path resolution rules of the `NIX_PATH`
/// environment variable.
///
/// This struct can be constructed by parsing a string using the [`FromStr`]
/// impl, or via [`str::parse`]. Nix `<...>` paths can then be resolved using
/// [`NixSearchPath::resolve`].
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct NixSearchPath {
    entries: Vec<NixSearchPathEntry>,
}

impl NixSearchPath {
    /// Attempt to resolve the given `path` within this [`NixSearchPath`] using the
    /// path resolution rules for `<...>`-style paths
    pub fn resolve<P, IO>(
        &self,
        io: IO,
        path: P,
    ) -> Result<Result<PathBuf, CatchableErrorKind>, ErrorKind>
    where
        P: AsRef<Path>,
        IO: AsRef<dyn EvalIO>,
    {
        let path = path.as_ref();
        for entry in &self.entries {
            if let Some(p) = entry.resolve(&io, path)? {
                return Ok(Ok(p));
            }
        }
        Ok(Err(CatchableErrorKind::NixPathResolution(format!(
            "path '{}' was not found in the Nix search path",
            path.display()
        ))))
    }
}

impl FromStr for NixSearchPath {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let entries = s
            .split(':')
            .map(|s| s.parse())
            .collect::<Result<Vec<_>, _>>()?;
        Ok(NixSearchPath { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod parse {
        use super::*;

        #[test]
        fn bare_paths() {
            assert_eq!(
                NixSearchPath::from_str("/foo/bar:/baz").unwrap(),
                NixSearchPath {
                    entries: vec![
                        NixSearchPathEntry::Path("/foo/bar".into()),
                        NixSearchPathEntry::Path("/baz".into())
                    ],
                }
            );
        }

        #[test]
        fn mixed_prefix_and_paths() {
            assert_eq!(
                NixSearchPath::from_str("nixpkgs=/my/nixpkgs:/etc/nixos").unwrap(),
                NixSearchPath {
                    entries: vec![
                        NixSearchPathEntry::Prefix {
                            prefix: "nixpkgs".into(),
                            path: "/my/nixpkgs".into()
                        },
                        NixSearchPathEntry::Path("/etc/nixos".into())
                    ],
                }
            );
        }
    }

    mod resolve {
        use crate::StdIO;
        use path_clean::PathClean;
        use std::env::current_dir;

        use super::*;

        #[test]
        fn simple_dir() {
            let nix_search_path = NixSearchPath::from_str("./.").unwrap();
            let io = Box::new(StdIO {}) as Box<dyn EvalIO>;
            let res = nix_search_path.resolve(&io, "src").unwrap();
            assert_eq!(
                res.unwrap().to_path_buf(),
                current_dir().unwrap().join("src").clean()
            );
        }

        #[test]
        fn failed_resolution() {
            let nix_search_path = NixSearchPath::from_str("./.").unwrap();
            let io = Box::new(StdIO {}) as Box<dyn EvalIO>;
            let err = nix_search_path.resolve(&io, "nope").unwrap();
            assert!(
                matches!(err, Err(CatchableErrorKind::NixPathResolution(..))),
                "err = {err:?}"
            );
        }

        #[test]
        fn second_in_path() {
            let nix_search_path = NixSearchPath::from_str("./.:/").unwrap();
            let io = Box::new(StdIO {}) as Box<dyn EvalIO>;
            let res = nix_search_path.resolve(&io, "etc").unwrap();
            assert_eq!(res.unwrap().to_path_buf(), Path::new("/etc"));
        }

        #[test]
        fn prefix() {
            let nix_search_path = NixSearchPath::from_str("/:tvix=.").unwrap();
            let io = Box::new(StdIO {}) as Box<dyn EvalIO>;
            let res = nix_search_path.resolve(&io, "tvix/src").unwrap();
            assert_eq!(
                res.unwrap().to_path_buf(),
                current_dir().unwrap().join("src").clean()
            );
        }

        #[test]
        fn matching_prefix() {
            let nix_search_path = NixSearchPath::from_str("/:tvix=.").unwrap();
            let io = Box::new(StdIO {}) as Box<dyn EvalIO>;
            let res = nix_search_path.resolve(&io, "tvix").unwrap();
            assert_eq!(res.unwrap().to_path_buf(), current_dir().unwrap().clean());
        }
    }
}
