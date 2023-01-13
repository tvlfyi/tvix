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
pub struct ReferenceScanner {
    candidates: Vec<String>,
    searcher: AhoCorasick,
    matches: Vec<usize>,
}

impl ReferenceScanner {
    /// Construct a new `ReferenceScanner` that knows how to scan for the given
    /// candidate store paths.
    pub fn new(candidates: Vec<String>) -> Self {
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
            self.matches.push(m.pattern());
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
            self.matches.push(m?.pattern());
        }

        Ok(())
    }

    /// Finalise the reference scanner and return the resulting matches.
    pub fn finalise(self) -> BTreeSet<String> {
        self.matches
            .into_iter()
            .map(|idx| self.candidates[idx].clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The actual derivation of `nixpkgs.hello`.
    const HELLO_DRV: &'static str = r#"Derive([("out","/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1","","")],[("/nix/store/6z1jfnqqgyqr221zgbpm30v91yfj3r45-bash-5.1-p16.drv",["out"]),("/nix/store/ap9g09fxbicj836zm88d56dn3ff4clxl-stdenv-linux.drv",["out"]),("/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv",["out"])],["/nix/store/9krlzvny65gdc8s7kpb6lkx8cd02c25b-default-builder.sh"],"x86_64-linux","/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16/bin/bash",["-e","/nix/store/9krlzvny65gdc8s7kpb6lkx8cd02c25b-default-builder.sh"],[("buildInputs",""),("builder","/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16/bin/bash"),("cmakeFlags",""),("configureFlags",""),("depsBuildBuild",""),("depsBuildBuildPropagated",""),("depsBuildTarget",""),("depsBuildTargetPropagated",""),("depsHostHost",""),("depsHostHostPropagated",""),("depsTargetTarget",""),("depsTargetTargetPropagated",""),("doCheck","1"),("doInstallCheck",""),("mesonFlags",""),("name","hello-2.12.1"),("nativeBuildInputs",""),("out","/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1"),("outputs","out"),("patches",""),("pname","hello"),("propagatedBuildInputs",""),("propagatedNativeBuildInputs",""),("src","/nix/store/pa10z4ngm0g83kx9mssrqzz30s84vq7k-hello-2.12.1.tar.gz"),("stdenv","/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux"),("strictDeps",""),("system","x86_64-linux"),("version","2.12.1")])"#;

    #[test]
    fn test_empty() {
        let mut scanner = ReferenceScanner::new(vec![]);
        scanner.scan_str("hello world");
        assert!(scanner.finalise().is_empty());
    }

    #[test]
    fn test_single_match() {
        let mut scanner = ReferenceScanner::new(vec![
            "/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16".into(),
        ]);
        scanner.scan_str(HELLO_DRV);

        let result = scanner.finalise();

        assert_eq!(result.len(), 1);
        assert!(result.contains("/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16"));
    }

    #[test]
    fn test_multiple_matches() {
        let candidates = vec![
            // these exist in the drv:
            "/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1".into(),
            "/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv".into(),
            "/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux".into(),
            // this doesn't:
            "/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-emacs-28.2.drv".into(),
        ];

        let mut scanner = ReferenceScanner::new(candidates.clone());
        scanner.scan_str(HELLO_DRV);

        let result = scanner.finalise();
        assert_eq!(result.len(), 3);

        for c in candidates[..3].iter() {
            assert!(result.contains(c));
        }
    }

    #[test]
    fn test_multiple_stream() {
        let candidates = vec![
            // these exist in the drv:
            "/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1".into(),
            "/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv".into(),
            "/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux".into(),
            // this doesn't:
            "/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-emacs-28.2.drv".into(),
        ];

        let mut scanner = ReferenceScanner::new(candidates.clone());
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