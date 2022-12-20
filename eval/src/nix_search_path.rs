use std::convert::Infallible;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::errors::ErrorKind;

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

impl NixSearchPathEntry {
    fn resolve(&self, lookup_path: &Path) -> io::Result<Option<PathBuf>> {
        let resolve_in =
            |parent: &Path, lookup_path: &Path| match parent.join(lookup_path).canonicalize() {
                Ok(path) => Ok(Some(path)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            };

        match self {
            NixSearchPathEntry::Path(p) => resolve_in(p, lookup_path),
            NixSearchPathEntry::Prefix { prefix, path } => {
                if let Ok(child_path) = lookup_path.strip_prefix(prefix) {
                    resolve_in(path, child_path)
                } else {
                    Ok(None)
                }
            }
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
    pub fn resolve<P>(&self, path: P) -> Result<PathBuf, ErrorKind>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        for entry in &self.entries {
            if let Some(p) = entry.resolve(path)? {
                return Ok(p);
            }
        }
        Err(ErrorKind::NixPathResolution(format!(
            "path '{}' was not found in the Nix search path",
            path.display()
        )))
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
        use std::env::current_dir;

        use path_clean::PathClean;

        use super::*;

        #[test]
        fn simple_dir() {
            let nix_search_path = NixSearchPath::from_str("./.").unwrap();
            let res = nix_search_path.resolve("src").unwrap();
            assert_eq!(res, current_dir().unwrap().join("src").clean());
        }

        #[test]
        fn failed_resolution() {
            let nix_search_path = NixSearchPath::from_str("./.").unwrap();
            let err = nix_search_path.resolve("nope").unwrap_err();
            assert!(
                matches!(err, ErrorKind::NixPathResolution(..)),
                "err = {err:?}"
            );
        }

        #[test]
        fn second_in_path() {
            let nix_search_path = NixSearchPath::from_str("./.:/").unwrap();
            let res = nix_search_path.resolve("etc").unwrap();
            assert_eq!(res, Path::new("/etc"));
        }

        #[test]
        fn prefix() {
            let nix_search_path = NixSearchPath::from_str("/:tvix=.").unwrap();
            let res = nix_search_path.resolve("tvix/src").unwrap();
            assert_eq!(res, current_dir().unwrap().join("src").clean());
        }

        #[test]
        fn matching_prefix() {
            let nix_search_path = NixSearchPath::from_str("/:tvix=.").unwrap();
            let res = nix_search_path.resolve("tvix").unwrap();
            assert_eq!(res, current_dir().unwrap().clean());
        }
    }
}
