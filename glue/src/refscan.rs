//! Simple scanner for non-overlapping, known references of Nix store paths in a
//! given string.
//!
//! This is used for determining build references (see
//! //tvix/eval/docs/build-references.md for more details).
//!
//! The scanner itself is using the Wu-Manber string-matching algorithm, using
//! our fork of the `wu-mamber` crate.
use pin_project::pin_project;
use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Poll};
use tokio::io::{AsyncBufRead, AsyncRead, ReadBuf};
use wu_manber::TwoByteWM;

/// A searcher that incapsulates the candidates and the Wu-Manber searcher.
/// This is separate from the scanner because we need to look for the same
/// pattern in multiple outputs and don't want to pay the price of constructing
/// the searcher for each build output.
pub struct ReferencePatternInner<P> {
    candidates: Vec<P>,
    longest_candidate: usize,
    // FUTUREWORK: Support overlapping patterns to be compatible with cpp Nix
    searcher: Option<TwoByteWM>,
}

#[derive(Clone)]
pub struct ReferencePattern<P> {
    inner: Arc<ReferencePatternInner<P>>,
}

impl<P> ReferencePattern<P> {
    pub fn candidates(&self) -> &[P] {
        &self.inner.candidates
    }

    pub fn longest_candidate(&self) -> usize {
        self.inner.longest_candidate
    }
}

impl<P: AsRef<[u8]>> ReferencePattern<P> {
    /// Construct a new `ReferencePattern` that knows how to scan for the given
    /// candidates.
    pub fn new(candidates: Vec<P>) -> Self {
        let searcher = if candidates.is_empty() {
            None
        } else {
            Some(TwoByteWM::new(&candidates))
        };
        let longest_candidate = candidates.iter().fold(0, |v, c| v.max(c.as_ref().len()));

        ReferencePattern {
            inner: Arc::new(ReferencePatternInner {
                searcher,
                candidates,
                longest_candidate,
            }),
        }
    }
}

impl<P> From<Vec<P>> for ReferencePattern<P>
where
    P: AsRef<[u8]>,
{
    fn from(candidates: Vec<P>) -> Self {
        Self::new(candidates)
    }
}

/// Represents a "primed" reference scanner with an automaton that knows the set
/// of bytes patterns to scan for.
pub struct ReferenceScanner<P> {
    pattern: ReferencePattern<P>,
    matches: Vec<bool>,
}

impl<P: AsRef<[u8]>> ReferenceScanner<P> {
    /// Construct a new `ReferenceScanner` that knows how to scan for the given
    /// candidate bytes patterns.
    pub fn new<IP: Into<ReferencePattern<P>>>(pattern: IP) -> Self {
        let pattern = pattern.into();
        let matches = vec![false; pattern.candidates().len()];
        ReferenceScanner { pattern, matches }
    }

    /// Scan the given buffer for all non-overlapping matches and collect them
    /// in the scanner.
    pub fn scan<S: AsRef<[u8]>>(&mut self, haystack: S) {
        if haystack.as_ref().len() < self.pattern.longest_candidate() {
            return;
        }

        if let Some(searcher) = &self.pattern.inner.searcher {
            for m in searcher.find(haystack) {
                self.matches[m.pat_idx] = true;
            }
        }
    }

    pub fn pattern(&self) -> &ReferencePattern<P> {
        &self.pattern
    }

    pub fn matches(&self) -> &[bool] {
        &self.matches
    }

    pub fn candidate_matches(&self) -> impl Iterator<Item = &P> {
        let candidates = self.pattern.candidates();
        self.matches.iter().enumerate().filter_map(|(idx, found)| {
            if *found {
                Some(&candidates[idx])
            } else {
                None
            }
        })
    }
}

impl<P: Clone + Ord + AsRef<[u8]>> ReferenceScanner<P> {
    /// Finalise the reference scanner and return the resulting matches.
    pub fn finalise(self) -> BTreeSet<P> {
        self.candidate_matches().cloned().collect()
    }
}

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

#[pin_project]
pub struct ReferenceReader<P, R> {
    scanner: ReferenceScanner<P>,
    buffer: Vec<u8>,
    consumed: usize,
    #[pin]
    reader: R,
}

impl<P, R> ReferenceReader<P, R>
where
    P: AsRef<[u8]>,
{
    pub fn new(pattern: ReferencePattern<P>, reader: R) -> ReferenceReader<P, R> {
        Self::with_capacity(DEFAULT_BUF_SIZE, pattern, reader)
    }

    pub fn with_capacity(
        capacity: usize,
        pattern: ReferencePattern<P>,
        reader: R,
    ) -> ReferenceReader<P, R> {
        // If capacity is not at least as long as longest_candidate we can't do a scan
        let capacity = capacity.max(pattern.longest_candidate());
        ReferenceReader {
            scanner: ReferenceScanner::new(pattern),
            buffer: Vec::with_capacity(capacity),
            consumed: 0,
            reader,
        }
    }

    pub fn scanner(&self) -> &ReferenceScanner<P> {
        &self.scanner
    }
}

impl<P, R> ReferenceReader<P, R>
where
    P: Clone + Ord + AsRef<[u8]>,
{
    pub fn finalise(self) -> BTreeSet<P> {
        self.scanner.finalise()
    }
}

impl<P, R> AsyncRead for ReferenceReader<P, R>
where
    R: AsyncRead,
    P: AsRef<[u8]>,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let internal_buf = ready!(self.as_mut().poll_fill_buf(cx))?;
        let amt = buf.remaining().min(internal_buf.len());
        buf.put_slice(&internal_buf[..amt]);
        self.consume(amt);
        Poll::Ready(Ok(()))
    }
}

impl<P, R> AsyncBufRead for ReferenceReader<P, R>
where
    R: AsyncRead,
    P: AsRef<[u8]>,
{
    fn poll_fill_buf(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<&[u8]>> {
        let overlap = self.scanner.pattern.longest_candidate() - 1;
        let mut this = self.project();
        // Still data in buffer
        if *this.consumed < this.buffer.len() {
            return Poll::Ready(Ok(&this.buffer[*this.consumed..]));
        }
        // We need to copy last `overlap` bytes to front to deal with references that overlap reads
        if *this.consumed > overlap {
            let start = this.buffer.len() - overlap;
            this.buffer.copy_within(start.., 0);
            this.buffer.truncate(overlap);
            *this.consumed = overlap;
        }
        // Read at least until self.buffer.len() > overlap so we can do one scan
        loop {
            let filled = {
                let mut buf = ReadBuf::uninit(this.buffer.spare_capacity_mut());
                ready!(this.reader.as_mut().poll_read(cx, &mut buf))?;
                buf.filled().len()
            };
            // SAFETY: We just read `filled` amount of data above
            unsafe {
                this.buffer.set_len(filled + this.buffer.len());
            }
            if filled == 0 || this.buffer.len() > overlap {
                break;
            }
        }
        this.scanner.scan(&this.buffer);
        Poll::Ready(Ok(&this.buffer[*this.consumed..]))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        debug_assert!(self.consumed + amt <= self.buffer.len());
        let this = self.project();
        *this.consumed += amt;
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tokio::io::AsyncReadExt as _;
    use tokio_test::io::Builder;

    use super::*;

    // The actual derivation of `nixpkgs.hello`.
    const HELLO_DRV: &str = r#"Derive([("out","/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1","","")],[("/nix/store/6z1jfnqqgyqr221zgbpm30v91yfj3r45-bash-5.1-p16.drv",["out"]),("/nix/store/ap9g09fxbicj836zm88d56dn3ff4clxl-stdenv-linux.drv",["out"]),("/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv",["out"])],["/nix/store/9krlzvny65gdc8s7kpb6lkx8cd02c25b-default-builder.sh"],"x86_64-linux","/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16/bin/bash",["-e","/nix/store/9krlzvny65gdc8s7kpb6lkx8cd02c25b-default-builder.sh"],[("buildInputs",""),("builder","/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16/bin/bash"),("cmakeFlags",""),("configureFlags",""),("depsBuildBuild",""),("depsBuildBuildPropagated",""),("depsBuildTarget",""),("depsBuildTargetPropagated",""),("depsHostHost",""),("depsHostHostPropagated",""),("depsTargetTarget",""),("depsTargetTargetPropagated",""),("doCheck","1"),("doInstallCheck",""),("mesonFlags",""),("name","hello-2.12.1"),("nativeBuildInputs",""),("out","/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1"),("outputs","out"),("patches",""),("pname","hello"),("propagatedBuildInputs",""),("propagatedNativeBuildInputs",""),("src","/nix/store/pa10z4ngm0g83kx9mssrqzz30s84vq7k-hello-2.12.1.tar.gz"),("stdenv","/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux"),("strictDeps",""),("system","x86_64-linux"),("version","2.12.1")])"#;

    #[test]
    fn test_no_patterns() {
        let mut scanner: ReferenceScanner<String> = ReferenceScanner::new(vec![]);

        scanner.scan(HELLO_DRV);

        let result = scanner.finalise();

        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_single_match() {
        let mut scanner = ReferenceScanner::new(vec![
            "/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16".to_string(),
        ]);
        scanner.scan(HELLO_DRV);

        let result = scanner.finalise();

        assert_eq!(result.len(), 1);
        assert!(result.contains("/nix/store/4xw8n979xpivdc46a9ndcvyhwgif00hz-bash-5.1-p16"));
    }

    #[test]
    fn test_multiple_matches() {
        let candidates = vec![
            // these exist in the drv:
            "/nix/store/33l4p0pn0mybmqzaxfkpppyh7vx1c74p-hello-2.12.1".to_string(),
            "/nix/store/pf80kikyxr63wrw56k00i1kw6ba76qik-hello-2.12.1.tar.gz.drv".to_string(),
            "/nix/store/cp65c8nk29qq5cl1wyy5qyw103cwmax7-stdenv-linux".to_string(),
            // this doesn't:
            "/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-emacs-28.2.drv".to_string(),
        ];

        let mut scanner = ReferenceScanner::new(candidates.clone());
        scanner.scan(HELLO_DRV);

        let result = scanner.finalise();
        assert_eq!(result.len(), 3);

        for c in candidates[..3].iter() {
            assert!(result.contains(c));
        }
    }

    #[rstest]
    #[case::normal(8096, 8096)]
    #[case::small_capacity(8096, 1)]
    #[case::small_read(1, 8096)]
    #[case::all_small(1, 1)]
    #[tokio::test]
    async fn test_reference_reader(#[case] chunk_size: usize, #[case] capacity: usize) {
        let candidates = vec![
            // these exist in the drv:
            "33l4p0pn0mybmqzaxfkpppyh7vx1c74p",
            "pf80kikyxr63wrw56k00i1kw6ba76qik",
            "cp65c8nk29qq5cl1wyy5qyw103cwmax7",
            // this doesn't:
            "fn7zvafq26f0c8b17brs7s95s10ibfzs",
        ];
        let pattern = ReferencePattern::new(candidates.clone());
        let mut mock = Builder::new();
        for c in HELLO_DRV.as_bytes().chunks(chunk_size) {
            mock.read(c);
        }
        let mock = mock.build();
        let mut reader = ReferenceReader::with_capacity(capacity, pattern, mock);
        let mut s = String::new();
        reader.read_to_string(&mut s).await.unwrap();
        assert_eq!(s, HELLO_DRV);

        let result = reader.finalise();
        assert_eq!(result.len(), 3);

        for c in candidates[..3].iter() {
            assert!(result.contains(c));
        }
    }

    // FUTUREWORK: Test with large file
}
