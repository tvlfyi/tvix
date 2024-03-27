//! This contains test scenarios that a given [BlobService] needs to pass.
//! We use [rstest] and [rstest_reuse] to provide all services we want to test
//! against, and then apply this template to all test functions.

use rstest::*;
use rstest_reuse::{self, *};
use std::io;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncSeekExt;

use super::BlobService;
use crate::blobservice;
use crate::fixtures::BLOB_A;
use crate::fixtures::BLOB_A_DIGEST;
use crate::fixtures::BLOB_B;
use crate::fixtures::BLOB_B_DIGEST;

mod utils;
use self::utils::make_grpc_blob_service_client;

/// This produces a template, which will be applied to all individual test functions.
/// See https://github.com/la10736/rstest/issues/130#issuecomment-968864832
#[template]
#[rstest]
#[case::grpc(make_grpc_blob_service_client().await)]
#[case::memory(blobservice::from_addr("memory://").await.unwrap())]
#[case::objectstore_memory(blobservice::from_addr("objectstore+memory://").await.unwrap())]
#[case::sled(blobservice::from_addr("sled://").await.unwrap())]
pub fn blob_services(#[case] blob_service: impl BlobService) {}

/// Using [BlobService::has] on a non-existing blob should return false.
#[apply(blob_services)]
#[tokio::test]
async fn has_nonexistent_false(blob_service: impl BlobService) {
    assert!(!blob_service
        .has(&BLOB_A_DIGEST)
        .await
        .expect("must not fail"));
}

/// Using [BlobService::chunks] on a non-existing blob should return Ok(None)
#[apply(blob_services)]
#[tokio::test]
async fn chunks_nonexistent_false(blob_service: impl BlobService) {
    assert!(blob_service
        .chunks(&BLOB_A_DIGEST)
        .await
        .expect("must be ok")
        .is_none());
}

// TODO: do tests with `chunks`

/// Trying to read a non-existing blob should return a None instead of a reader.
#[apply(blob_services)]
#[tokio::test]
async fn not_found_read(blob_service: impl BlobService) {
    assert!(blob_service
        .open_read(&BLOB_A_DIGEST)
        .await
        .expect("must not fail")
        .is_none())
}

/// Put a blob in the store, check has, get it back.
#[apply(blob_services)]
// #[case::small(&fixtures::BLOB_A, &fixtures::BLOB_A_DIGEST)]
// #[case::big(&fixtures::BLOB_B, &fixtures::BLOB_B_DIGEST)]
#[tokio::test]
async fn put_has_get(blob_service: impl BlobService) {
    // TODO: figure out how to instantiate this with BLOB_A and BLOB_B, as two separate cases
    for (blob_contents, blob_digest) in &[
        (&*BLOB_A, BLOB_A_DIGEST.clone()),
        (&*BLOB_B, BLOB_B_DIGEST.clone()),
    ] {
        let mut w = blob_service.open_write().await;

        let l = tokio::io::copy(&mut io::Cursor::new(blob_contents), &mut w)
            .await
            .expect("copy must succeed");
        assert_eq!(
            blob_contents.len(),
            l as usize,
            "written bytes must match blob length"
        );

        let digest = w.close().await.expect("close must succeed");

        assert_eq!(*blob_digest, digest, "returned digest must be correct");

        assert!(
            blob_service.has(blob_digest).await.expect("must not fail"),
            "blob service should now have the blob"
        );

        let mut r = blob_service
            .open_read(blob_digest)
            .await
            .expect("open_read must succeed")
            .expect("must be some");

        let mut buf: Vec<u8> = Vec::new();
        let mut pinned_reader = std::pin::pin!(r);
        let l = tokio::io::copy(&mut pinned_reader, &mut buf)
            .await
            .expect("copy must succeed");

        assert_eq!(
            blob_contents.len(),
            l as usize,
            "read bytes must match blob length"
        );

        assert_eq!(&blob_contents[..], &buf, "read blob contents must match");
    }
}

/// Put a blob in the store, and seek inside it a bit.
#[apply(blob_services)]
#[tokio::test]
async fn put_seek(blob_service: impl BlobService) {
    let mut w = blob_service.open_write().await;

    tokio::io::copy(&mut io::Cursor::new(&BLOB_B.to_vec()), &mut w)
        .await
        .expect("copy must succeed");
    w.close().await.expect("close must succeed");

    // open a blob for reading
    let mut r = blob_service
        .open_read(&BLOB_B_DIGEST)
        .await
        .expect("open_read must succeed")
        .expect("must be some");

    let mut pos: u64 = 0;

    // read the first 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).await.expect("must succeed");

        assert_eq!(
            &BLOB_B[pos as usize..pos as usize + buf.len()],
            buf,
            "expected first 10 bytes to match"
        );

        pos += buf.len() as u64;
    }
    // seek by 0 bytes, using SeekFrom::Start.
    let p = r
        .seek(io::SeekFrom::Start(pos))
        .await
        .expect("must not fail");
    assert_eq!(pos, p);

    // read the next 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).await.expect("must succeed");

        assert_eq!(
            &BLOB_B[pos as usize..pos as usize + buf.len()],
            buf,
            "expected data to match"
        );

        pos += buf.len() as u64;
    }

    // seek by 5 bytes, using SeekFrom::Start.
    let p = r
        .seek(io::SeekFrom::Start(pos + 5))
        .await
        .expect("must not fail");
    pos += 5;
    assert_eq!(pos, p);

    // read the next 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).await.expect("must succeed");

        assert_eq!(
            &BLOB_B[pos as usize..pos as usize + buf.len()],
            buf,
            "expected data to match"
        );

        pos += buf.len() as u64;
    }

    // seek by 12345 bytes, using SeekFrom::
    let p = r
        .seek(io::SeekFrom::Current(12345))
        .await
        .expect("must not fail");
    pos += 12345;
    assert_eq!(pos, p);

    // read the next 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).await.expect("must succeed");

        assert_eq!(
            &BLOB_B[pos as usize..pos as usize + buf.len()],
            buf,
            "expected data to match"
        );

        #[allow(unused_assignments)]
        {
            pos += buf.len() as u64;
        }
    }

    // seeking to the end is okay…
    let p = r
        .seek(io::SeekFrom::Start(BLOB_B.len() as u64))
        .await
        .expect("must not fail");
    pos = BLOB_B.len() as u64;
    assert_eq!(pos, p);

    {
        // but it returns no more data.
        let mut buf: Vec<u8> = Vec::new();
        r.read_to_end(&mut buf).await.expect("must not fail");
        assert!(buf.is_empty(), "expected no more data to be read");
    }

    // seeking past the end…
    // should either be ok, but then return 0 bytes.
    // this matches the behaviour or a Cursor<Vec<u8>>.
    if let Ok(_pos) = r.seek(io::SeekFrom::Start(BLOB_B.len() as u64 + 1)).await {
        let mut buf: Vec<u8> = Vec::new();
        r.read_to_end(&mut buf).await.expect("must not fail");
        assert!(buf.is_empty(), "expected no more data to be read");
    }
    // or not be okay.

    // TODO: this is only broken for the gRPC version
    // We expect seeking backwards or relative to the end to fail.
    // r.seek(io::SeekFrom::Current(-1))
    //     .expect_err("SeekFrom::Current(-1) expected to fail");

    // r.seek(io::SeekFrom::Start(pos - 1))
    //     .expect_err("SeekFrom::Start(pos-1) expected to fail");

    // r.seek(io::SeekFrom::End(0))
    //     .expect_err("SeekFrom::End(_) expected to fail");
}
