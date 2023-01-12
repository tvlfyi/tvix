//! Simple scanner for non-overlapping, known references of Nix store paths in a
//! given string.
//!
//! This is used for determining build references (see
//! //tvix/eval/docs/build-references.md for more details).
//!
//! The scanner itself is an Aho-Corasick automaton, using the `aho-corasick`
//! crate.

use aho_corasick::AhoCorasick;
use std::collections::BTreeSet;
use std::io;

/// Represents a "primed" reference scanner with an automaton that knows the set
/// of store paths to scan for.
pub struct ReferenceScanner<'s> {
    candidates: Vec<&'s str>,
    searcher: AhoCorasick,
    matches: BTreeSet<&'s str>,
}

pub trait ToOwnedVec<T> {
    fn to_owned_vec(self) -> Vec<T>;
}

impl<T: Clone> ToOwnedVec<T> for &[T] {
    fn to_owned_vec(self) -> Vec<T> {
        self.to_vec()
    }
}

impl<T> ToOwnedVec<T> for Vec<T> {
    fn to_owned_vec(self) -> Vec<T> {
        self
    }
}

impl<'s> ReferenceScanner<'s> {
    /// Construct a new `ReferenceScanner` that knows how to scan for the given
    /// candidate store paths.
    pub fn new<V: ToOwnedVec<&'s str>>(candidates: V) -> Self {
        let candidates = candidates.to_owned_vec();
        let searcher = AhoCorasick::new_auto_configured(&candidates);

        ReferenceScanner {
            searcher,
            candidates,
            matches: Default::default(),
        }
    }

    /// Scan the given string for all non-overlapping matches and collect them
    /// in the scanner.
    pub fn scan_str<H: AsRef<[u8]>>(&mut self, haystack: H) {
        for m in self.searcher.find_iter(&haystack) {
            let needle = self.candidates[m.pattern()];
            self.matches.insert(needle);
        }
    }

    /// Scan the given reader for all non-overlapping matches, and collect them
    /// in the scanner. On read failures, this method aborts and returns an
    /// error to the caller.
    ///
    /// Please note that the internal machinery has its own buffering mechanism,
    /// and where possible the given reader should be unbuffered. See
    /// [`AhoCorasick::stream_find_iter`] for details on this.
    pub fn scan_stream<R: io::Read>(&mut self, stream: R) -> io::Result<()> {
        for m in self.searcher.stream_find_iter(stream) {
            let needle = self.candidates[m?.pattern()];
            self.matches.insert(needle);
        }

        Ok(())
    }

    /// Finalise the reference scanner and return the resulting matches.
    pub fn finalise(self) -> BTreeSet<&'s str> {
        self.matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The actual derivation of `nixpkgs.hello`.
    const HELLO_DRV: &'static str = r#"Derive([("out","/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1","","")],[("/nix/store/6z1jfnqqgyqr221zgbpm30v91yfj3r45-bash-5.1-p16.drv",["out"]),("/nix/store/ap9g09fxbicj836zm88d56dn3ff4clxl-stdenv-linux.drv",["out"]),("/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv",["out"])],["/nix/store/9krlzvny65gdc8s7kpb6lkx8cd02c25b-default-builder.sh"],"x86_64-linux","/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16/bin/bash",["-e","/nix/store/9krlzvny65gdc8s7kpb6lkx8cd02c25b-default-builder.sh"],[("buildInputs",""),("builder","/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16/bin/bash"),("cmakeFlags",""),("configureFlags",""),("depsBuildBuild",""),("depsBuildBuildPropagated",""),("depsBuildTarget",""),("depsBuildTargetPropagated",""),("depsHostHost",""),("depsHostHostPropagated",""),("depsTargetTarget",""),("depsTargetTargetPropagated",""),("doCheck","1"),("doInstallCheck",""),("mesonFlags",""),("name","hello-2.12.1"),("nativeBuildInputs",""),("out","/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1"),("outputs","out"),("patches",""),("pname","hello"),("propagatedBuildInputs",""),("propagatedNativeBuildInputs",""),("src","/nix/store/pa10z4ngm0g83kx9mssrqzz30s84vq7k-hello-2.12.1.tar.gz"),("stdenv","/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux"),("strictDeps",""),("system","x86_64-linux"),("version","2.12.1")])"#;

    impl<T: Clone, const N: usize> ToOwnedVec<T> for &[T; N] {
        fn to_owned_vec(self) -> Vec<T> {
            self.to_vec()
        }
    }

    #[test]
    fn test_empty() {
        let mut scanner = ReferenceScanner::new(&[]);
        scanner.scan_str("hello world");
        assert!(scanner.finalise().is_empty());
    }

    #[test]
    fn test_single_match() {
        let mut scanner =
            ReferenceScanner::new(&["/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16"]);
        scanner.scan_str(HELLO_DRV);

        let result = scanner.finalise();

        assert_eq!(result.len(), 1);
        assert!(result.contains("/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16"));
    }

    #[test]
    fn test_multiple_matches() {
        let candidates = &[
            // these exist in the drv:
            "/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1",
            "/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv",
            "/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux",
            // this doesn't:
            "/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-emacs-28.2.drv",
        ];

        let mut scanner = ReferenceScanner::new(candidates);
        scanner.scan_str(HELLO_DRV);

        let result = scanner.finalise();
        assert_eq!(result.len(), 3);

        for c in candidates[..3].iter() {
            assert!(result.contains(c));
        }
    }

    #[test]
    fn test_multiple_stream() {
        let candidates = &[
            // these exist in the drv:
            "/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1",
            "/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv",
            "/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux",
            // this doesn't:
            "/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-emacs-28.2.drv",
        ];

        let mut scanner = ReferenceScanner::new(candidates);
        scanner
            .scan_stream(HELLO_DRV.as_bytes())
            .expect("scanning should succeed");

        let result = scanner.finalise();
        assert_eq!(result.len(), 3);

        for c in candidates[..3].iter() {
            assert!(result.contains(c));
        }
    }
}
