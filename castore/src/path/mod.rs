//! Contains data structures to deal with Paths in the tvix-castore model.
use bstr::ByteSlice;
use std::{
    borrow::Borrow,
    fmt::{self, Debug, Display},
    mem,
    ops::Deref,
    str::FromStr,
};

mod component;
pub use component::{PathComponent, PathComponentError};

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
                if component::validate_name(component).is_err() {
                    return None;
                }
            }
        }

        // SAFETY: We have verified that the path contains no empty components.
        Some(unsafe { Path::from_bytes_unchecked(bytes) })
    }

    pub fn into_boxed_bytes(self: Box<Path>) -> Box<[u8]> {
        // SAFETY: Box<Path> and Box<[u8]> have the same representation.
        unsafe { mem::transmute(self) }
    }

    /// Returns the path without its final component, if there is one.
    ///
    /// Note that the parent of a bare file name is [Path::ROOT].
    /// [Path::ROOT] is the only path without a parent.
    pub fn parent(&self) -> Option<&Path> {
        // The root does not have a parent.
        if self.inner.is_empty() {
            return None;
        }

        Some(
            if let Some((parent, _file_name)) = self.inner.rsplit_once_str(b"/") {
                // SAFETY: The parent of a valid Path is a valid Path.
                unsafe { Path::from_bytes_unchecked(parent) }
            } else {
                // The parent of a bare file name is the root.
                Path::ROOT
            },
        )
    }

    /// Creates a PathBuf with `name` adjoined to self.
    pub fn try_join(&self, name: &[u8]) -> Result<PathBuf, std::io::Error> {
        let mut v = PathBuf::with_capacity(self.inner.len() + name.len() + 1);
        v.inner.extend_from_slice(&self.inner);
        v.try_push(name)?;

        Ok(v)
    }

    /// Provides an iterator over the components of the path,
    /// which are invividual [PathComponent].
    /// In case the path is empty, an empty iterator is returned.
    pub fn components(&self) -> impl Iterator<Item = PathComponent> + '_ {
        let mut iter = self.inner.split_str(&b"/");

        // We don't want to return an empty element, consume it if it's the only one.
        if self.inner.is_empty() {
            let _ = iter.next();
        }

        iter.map(|b| PathComponent {
            inner: bytes::Bytes::copy_from_slice(b),
        })
    }

    /// Produces an iterator over the components of the path, which are
    /// individual byte slices.
    /// In case the path is empty, an empty iterator is returned.
    pub fn components_bytes(&self) -> impl Iterator<Item = &[u8]> {
        let mut iter = self.inner.split_str(&b"/");

        // We don't want to return an empty element, consume it if it's the only one.
        if self.inner.is_empty() {
            let _ = iter.next();
        }

        iter
    }

    /// Returns the final component of the Path, if there is one, in bytes.
    pub fn file_name(&self) -> Option<PathComponent> {
        self.components().last()
    }

    /// Returns the final component of the Path, if there is one, in bytes.
    pub fn file_name_bytes(&self) -> Option<&[u8]> {
        self.components_bytes().last()
    }

    pub fn as_bytes(&self) -> &[u8] {
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

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
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

impl From<Box<Path>> for PathBuf {
    fn from(value: Box<Path>) -> Self {
        // SAFETY: Box<Path> is always a valid path.
        unsafe { PathBuf::from_bytes_unchecked(value.into_boxed_bytes().into_vec()) }
    }
}

impl From<&Path> for PathBuf {
    fn from(value: &Path) -> Self {
        value.to_owned()
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

impl PathBuf {
    pub fn new() -> PathBuf {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> PathBuf {
        // SAFETY: The empty path is a valid path.
        Self {
            inner: Vec::with_capacity(capacity),
        }
    }

    /// Adjoins `name` to self.
    pub fn try_push(&mut self, name: &[u8]) -> Result<(), std::io::Error> {
        if component::validate_name(name).is_err() {
            return Err(std::io::ErrorKind::InvalidData.into());
        }

        if !self.inner.is_empty() {
            self.inner.push(b'/');
        }

        self.inner.extend_from_slice(name);

        Ok(())
    }

    /// Convert a byte vector to a PathBuf, without checking validity.
    unsafe fn from_bytes_unchecked(bytes: Vec<u8>) -> PathBuf {
        PathBuf { inner: bytes }
    }

    /// Convert from a [&std::path::Path] to [Self].
    ///
    /// - Self uses `/` as path separator.
    /// - Absolute paths are always rejected, are are these with custom prefixes.
    /// - Repeated separators are deduplicated.
    /// - Occurrences of `.` are normalized away.
    /// - A trailing slash is normalized away.
    ///
    /// A `canonicalize_dotdot` boolean controls whether `..` will get
    /// canonicalized if possible, or should return an error.
    ///
    /// For more exotic paths, this conversion might produce different results
    /// on different platforms, due to different underlying byte
    /// representations, which is why it's restricted to unix for now.
    #[cfg(unix)]
    pub fn from_host_path(
        host_path: &std::path::Path,
        canonicalize_dotdot: bool,
    ) -> Result<Self, std::io::Error> {
        let mut p = PathBuf::with_capacity(host_path.as_os_str().len());

        for component in host_path.components() {
            match component {
                std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "found disallowed prefix or rootdir",
                    ))
                }
                std::path::Component::CurDir => continue, // ignore
                std::path::Component::ParentDir => {
                    if canonicalize_dotdot {
                        // Try popping the last element from the path being constructed.
                        // FUTUREWORK: pop method?
                        p = p
                            .parent()
                            .ok_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "found .. going too far up",
                                )
                            })?
                            .to_owned();
                    } else {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "found disallowed ..",
                        ));
                    }
                }
                std::path::Component::Normal(s) => {
                    // append the new component to the path being constructed.
                    p.try_push(s.as_encoded_bytes()).map_err(|_| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "encountered invalid node in sub_path component",
                        )
                    })?
                }
            }
        }

        Ok(p)
    }

    pub fn into_boxed_path(self) -> Box<Path> {
        // SAFETY: Box<[u8]> and Box<Path> have the same representation,
        // and PathBuf always contains a valid Path.
        unsafe { mem::transmute(self.inner.into_boxed_slice()) }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.inner
    }
}

#[cfg(test)]
mod test {
    use super::{Path, PathBuf};
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
    #[case::cursed("\\\\tvix-store", 1)]
    pub fn from_str(#[case] s: &str, #[case] num_components: usize) {
        let p: PathBuf = s.parse().expect("must parse");

        assert_eq!(s.as_bytes(), p.as_bytes(), "inner bytes mismatch");
        assert_eq!(
            num_components,
            p.components_bytes().count(),
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
    #[case("foo", "")]
    #[case("foo/bar", "foo")]
    #[case("foo2/bar2", "foo2")]
    #[case("foo/bar/baz", "foo/bar")]
    pub fn parent(#[case] p: PathBuf, #[case] exp_parent: PathBuf) {
        assert_eq!(Some(&*exp_parent), p.parent());
    }

    #[rstest]
    pub fn no_parent() {
        assert!(Path::ROOT.parent().is_none());
    }

    #[rstest]
    #[case("a", "b", "a/b")]
    #[case("a", "b", "a/b")]
    pub fn join_push(#[case] mut p: PathBuf, #[case] name: &str, #[case] exp_p: PathBuf) {
        assert_eq!(exp_p, p.try_join(name.as_bytes()).expect("join failed"));
        p.try_push(name.as_bytes()).expect("push failed");
        assert_eq!(exp_p, p);
    }

    #[rstest]
    #[case("a", "/")]
    #[case("a", "")]
    #[case("a", "b/c")]
    #[case("", "/")]
    #[case("", "")]
    #[case("", "b/c")]
    #[case("", ".")]
    #[case("", "..")]
    pub fn join_push_fail(#[case] mut p: PathBuf, #[case] name: &str) {
        p.try_join(name.as_bytes())
            .expect_err("join succeeded unexpectedly");
        p.try_push(name.as_bytes())
            .expect_err("push succeeded unexpectedly");
    }

    #[rstest]
    #[case::empty("", vec![])]
    #[case("a", vec!["a"])]
    #[case("a/b", vec!["a", "b"])]
    #[case("a/b/c", vec!["a","b", "c"])]
    pub fn components_bytes(#[case] p: PathBuf, #[case] exp_components: Vec<&str>) {
        assert_eq!(
            exp_components,
            p.components_bytes()
                .map(|x| x.to_str().unwrap())
                .collect::<Vec<_>>()
        );
    }

    #[rstest]
    #[case::empty("", "", false)]
    #[case::path("a", "a", false)]
    #[case::path2("a/b", "a/b", false)]
    #[case::double_slash_middle("a//b", "a/b", false)]
    #[case::dot(".", "", false)]
    #[case::dot_start("./a/b", "a/b", false)]
    #[case::dot_middle("a/./b", "a/b", false)]
    #[case::dot_end("a/b/.", "a/b", false)]
    #[case::trailing_slash("a/b/", "a/b", false)]
    #[case::dotdot_canonicalize("a/..", "", true)]
    #[case::dotdot_canonicalize2("a/../b", "b", true)]
    #[cfg_attr(unix, case::faux_prefix("\\\\nix-store", "\\\\nix-store", false))]
    #[cfg_attr(unix, case::faux_letter("C:\\foo.txt", "C:\\foo.txt", false))]
    pub fn from_host_path(
        #[case] host_path: std::path::PathBuf,
        #[case] exp_path: PathBuf,
        #[case] canonicalize_dotdot: bool,
    ) {
        let p = PathBuf::from_host_path(&host_path, canonicalize_dotdot).expect("must succeed");

        assert_eq!(exp_path, p);
    }

    #[rstest]
    #[case::absolute("/", false)]
    #[case::dotdot_root("..", false)]
    #[case::dotdot_root_canonicalize("..", true)]
    #[case::dotdot_root_no_canonicalize("a/..", false)]
    #[case::invalid_name("foo/bar\0", false)]
    // #[cfg_attr(windows, case::prefix("\\\\nix-store", false))]
    // #[cfg_attr(windows, case::letter("C:\\foo.txt", false))]
    pub fn from_host_path_fail(
        #[case] host_path: std::path::PathBuf,
        #[case] canonicalize_dotdot: bool,
    ) {
        PathBuf::from_host_path(&host_path, canonicalize_dotdot).expect_err("must fail");
    }
}
