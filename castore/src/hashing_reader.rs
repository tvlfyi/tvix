use pin_project_lite::pin_project;
use tokio::io::AsyncRead;

pin_project! {
    /// Wraps an existing AsyncRead, and allows querying for the digest of all
    /// data read "through" it.
    /// The hash function is configurable by type parameter.
    pub struct HashingReader<R, H>
    where
        R: AsyncRead,
        H: digest::Digest,
    {
        #[pin]
        inner: R,
        hasher: H,
    }
}

pub type B3HashingReader<R> = HashingReader<R, blake3::Hasher>;

impl<R, H> HashingReader<R, H>
where
    R: AsyncRead,
    H: digest::Digest,
{
    pub fn from(r: R) -> Self {
        Self {
            inner: r,
            hasher: H::new(),
        }
    }

    /// Return the digest.
    pub fn digest(self) -> digest::Output<H> {
        self.hasher.finalize()
    }
}

impl<R, H> tokio::io::AsyncRead for HashingReader<R, H>
where
    R: AsyncRead,
    H: digest::Digest,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let buf_filled_len_before = buf.filled().len();

        let this = self.project();
        let ret = this.inner.poll_read(cx, buf);

        // write everything new filled into the hasher.
        this.hasher.update(&buf.filled()[buf_filled_len_before..]);

        ret
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use rstest::rstest;

    use crate::fixtures::BLOB_A;
    use crate::fixtures::BLOB_A_DIGEST;
    use crate::fixtures::BLOB_B;
    use crate::fixtures::BLOB_B_DIGEST;
    use crate::fixtures::EMPTY_BLOB_DIGEST;
    use crate::{B3Digest, B3HashingReader};

    #[rstest]
    #[case::blob_a(&BLOB_A, &BLOB_A_DIGEST)]
    #[case::blob_b(&BLOB_B, &BLOB_B_DIGEST)]
    #[case::empty_blob(&[], &EMPTY_BLOB_DIGEST)]
    #[tokio::test]
    async fn test_b3_hashing_reader(#[case] data: &[u8], #[case] b3_digest: &B3Digest) {
        let r = Cursor::new(data);
        let mut hr = B3HashingReader::from(r);

        tokio::io::copy(&mut hr, &mut tokio::io::sink())
            .await
            .expect("read must succeed");

        assert_eq!(*b3_digest, hr.digest().into());
    }
}
