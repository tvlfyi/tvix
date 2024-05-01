//! Contains data structures to deal with Paths in the tvix-castore model.

use std::{
    borrow::Borrow,
    fmt::{self, Debug, Display},
    mem,
    ops::Deref,
    str::FromStr,
};

use bstr::ByteSlice;

use crate::proto::validate_node_name;

/// Represents a Path in the castore model.
/// These are always relative, and platform-independent, which distinguishes
/// them from the ones provided in the standard library.
#[derive(Eq, Hash, PartialEq)]
#[repr(transparent)] // SAFETY: Representation has to match [u8]
pub struct Path {
    // As node names in the castore model cannot contain slashes,
    // we use them as component separators here.
    inner: [u8],
}

#[allow(dead_code)]
impl Path {
    // SAFETY: The empty path is valid.
    pub const ROOT: &'static Path = unsafe { Path::from_bytes_unchecked(&[]) };

    /// Convert a byte slice to a path, without checking validity.
    const unsafe fn from_bytes_unchecked(bytes: &[u8]) -> &Path {
        // SAFETY: &[u8] and &Path have the same representation.
        unsafe { mem::transmute(bytes) }
    }

    fn from_bytes(bytes: &[u8]) -> Option<&Path> {
        if !bytes.is_empty() {
            // Ensure all components are valid castore node names.
            for component in bytes.split_str(b"/") {
                validate_node_name(component).ok()?;
            }
        }

        // SAFETY: We have verified that the path contains no empty components.
        Some(unsafe { Path::from_bytes_unchecked(bytes) })
    }

    pub fn parent(&self) -> Option<&Path> {
        let (parent, _file_name) = self.inner.rsplit_once_str(b"/")?;

        // SAFETY: The parent of a valid Path is a valid Path.
        Some(unsafe { Path::from_bytes_unchecked(parent) })
    }

    pub fn join(&self, name: &[u8]) -> Result<PathBuf, std::io::Error> {
        if name.contains(&b'/') || name.is_empty() {
            return Err(std::io::ErrorKind::InvalidData.into());
        }

        let mut v = self.inner.to_vec();
        if !v.is_empty() {
            v.extend_from_slice(b"/");
        }
        v.extend_from_slice(name);

        Ok(PathBuf { inner: v })
    }

    /// Produces an iterator over the components of the path, which are
    /// individual byte slices.
    /// In case the path is empty, an empty iterator is returned.
    pub fn components(&self) -> impl Iterator<Item = &[u8]> {
        let mut iter = self.inner.split_str(&b"/");

        // We don't want to return an empty element, consume it if it's the only one.
        if self.inner.is_empty() {
            let _ = iter.next();
        }

        iter
    }

    /// Returns the final component of the Path, if there is one.
    pub fn file_name(&self) -> Option<&[u8]> {
        self.components().last()
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.inner
    }
}

impl Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(self.inner.as_bstr(), f)
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(self.inner.as_bstr(), f)
    }
}

/// Represents a owned PathBuf in the castore model.
/// These are always relative, and platform-independent, which distinguishes
/// them from the ones provided in the standard library.
#[derive(Clone, Default, Eq, Hash, PartialEq)]
pub struct PathBuf {
    inner: Vec<u8>,
}

impl Deref for PathBuf {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        // SAFETY: PathBuf always contains a valid Path.
        unsafe { Path::from_bytes_unchecked(&self.inner) }
    }
}

impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl ToOwned for Path {
    type Owned = PathBuf;

    fn to_owned(&self) -> Self::Owned {
        PathBuf {
            inner: self.inner.to_owned(),
        }
    }
}

impl Borrow<Path> for PathBuf {
    fn borrow(&self) -> &Path {
        self
    }
}

impl FromStr for PathBuf {
    type Err = std::io::Error;

    fn from_str(s: &str) -> Result<PathBuf, Self::Err> {
        Ok(Path::from_bytes(s.as_bytes())
            .ok_or(std::io::ErrorKind::InvalidData)?
            .to_owned())
    }
}

impl Debug for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(&**self, f)
    }
}

impl Display for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&**self, f)
    }
}

#[cfg(test)]
mod test {
    use super::PathBuf;
    use bstr::ByteSlice;
    use rstest::rstest;

    // TODO: add some manual tests including invalid UTF-8 (hard to express
    // with rstest)

    #[rstest]
    #[case::empty("", 0)]
    #[case("a", 1)]
    #[case("a/b", 2)]
    #[case("a/b/c", 3)]
    // add two slightly more cursed variants.
    // Technically nothing prevents us from representing this with castore,
    // but maybe we want to disallow constructing paths like this as it's a
    // bad idea.
    #[case::cursed("C:\\a/b", 2)]
    #[case::cursed("\\tvix-store", 1)]
    pub fn from_str(#[case] s: &str, #[case] num_components: usize) {
        let p: PathBuf = s.parse().expect("must parse");

        assert_eq!(s.as_bytes(), p.as_slice(), "inner bytes mismatch");
        assert_eq!(
            num_components,
            p.components().count(),
            "number of components mismatch"
        );
    }

    #[rstest]
    #[case::absolute("/a/b")]
    #[case::two_forward_slashes_start("//a/b")]
    #[case::two_forward_slashes_middle("a/b//c/d")]
    #[case::trailing_slash("a/b/")]
    #[case::dot(".")]
    #[case::dotdot("..")]
    #[case::dot_start("./a")]
    #[case::dotdot_start("../a")]
    #[case::dot_middle("a/./b")]
    #[case::dotdot_middle("a/../b")]
    #[case::dot_end("a/b/.")]
    #[case::dotdot_end("a/b/..")]
    #[case::null("fo\0o")]
    pub fn from_str_fail(#[case] s: &str) {
        s.parse::<PathBuf>().expect_err("must fail");
    }

    #[rstest]
    #[case("foo/bar", "foo")]
    #[case("foo2/bar2", "foo2")]
    #[case("foo/bar/baz", "foo/bar")]
    pub fn parent(#[case] p: PathBuf, #[case] exp_parent: PathBuf) {
        assert_eq!(Some(&*exp_parent), p.parent());
    }

    #[rstest]
    #[case::empty("")]
    #[case::single("foo")]
    pub fn no_parent(#[case] p: PathBuf) {
        assert!(p.parent().is_none());

        // same for Path
        assert!(p.as_ref().parent().is_none());
    }

    #[rstest]
    #[case("a", "b", "a/b")]
    #[case("a", "b", "a/b")]
    pub fn join(#[case] p: PathBuf, #[case] name: &str, #[case] exp_p: PathBuf) {
        assert_eq!(exp_p, p.join(name.as_bytes()).expect("join failed"));
    }

    #[rstest]
    #[case("a", "/")]
    #[case("a", "")]
    #[case("a", "b/c")]
    #[case("", "/")]
    #[case("", "")]
    #[case("", "b/c")]
    pub fn join_fail(#[case] p: PathBuf, #[case] name: &str) {
        p.join(name.as_bytes())
            .expect_err("join succeeded unexpectedly");
    }

    #[rstest]
    #[case::empty("", vec![])]
    #[case("a", vec!["a"])]
    #[case("a/b", vec!["a", "b"])]
    #[case("a/b/c", vec!["a","b", "c"])]
    pub fn components(#[case] p: PathBuf, #[case] exp_components: Vec<&str>) {
        assert_eq!(
            exp_components,
            p.components()
                .map(|x| x.to_str().unwrap())
                .collect::<Vec<_>>()
        );
    }
}
