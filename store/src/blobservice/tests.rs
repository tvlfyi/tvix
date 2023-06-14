use std::io;

use test_case::test_case;

use super::B3Digest;
use super::BlobService;
use super::MemoryBlobService;
use super::SledBlobService;
use crate::tests::fixtures;

// TODO: avoid having to define all different services we test against for all functions.
// maybe something like rstest can be used?

fn gen_memory_blob_service() -> impl BlobService {
    MemoryBlobService::default()
}
fn gen_sled_blob_service() -> impl BlobService {
    SledBlobService::new_temporary().unwrap()
}

// TODO: add GRPC blob service here.

/// Using [BlobService::has] on a non-existing blob should return false
#[test_case(gen_memory_blob_service(); "memory")]
#[test_case(gen_sled_blob_service(); "sled")]
fn has_nonexistent_false(blob_service: impl BlobService) {
    assert_eq!(
        blob_service
            .has(&fixtures::BLOB_A_DIGEST)
            .expect("must not fail"),
        false
    );
}

/// Trying to read a non-existing blob should return a None instead of a reader.
#[test_case(gen_memory_blob_service(); "memory")]
#[test_case(gen_sled_blob_service(); "sled")]
fn not_found_read(blob_service: impl BlobService) {
    assert!(blob_service
        .open_read(&fixtures::BLOB_A_DIGEST)
        .expect("must not fail")
        .is_none())
}

/// Put a blob in the store, check has, get it back.
/// We test both with small and big blobs.
#[test_case(gen_memory_blob_service(), &fixtures::BLOB_A, &fixtures::BLOB_A_DIGEST; "memory-small")]
#[test_case(gen_sled_blob_service(), &fixtures::BLOB_A, &fixtures::BLOB_A_DIGEST; "sled-small")]
#[test_case(gen_memory_blob_service(), &fixtures::BLOB_B, &fixtures::BLOB_B_DIGEST; "memory-big")]
#[test_case(gen_sled_blob_service(), &fixtures::BLOB_B, &fixtures::BLOB_B_DIGEST; "sled-big")]
fn put_has_get(blob_service: impl BlobService, blob_contents: &[u8], blob_digest: &B3Digest) {
    let mut w = blob_service.open_write();

    let l = io::copy(&mut io::Cursor::new(blob_contents), &mut w).expect("copy must succeed");
    assert_eq!(
        blob_contents.len(),
        l as usize,
        "written bytes must match blob length"
    );

    let digest = w.close().expect("close must succeed");

    assert_eq!(*blob_digest, digest, "returned digest must be correct");

    assert_eq!(
        blob_service.has(blob_digest).expect("must not fail"),
        true,
        "blob service should now have the blob"
    );

    let mut r = blob_service
        .open_read(blob_digest)
        .expect("open_read must succeed")
        .expect("must be some");

    let mut buf: Vec<u8> = Vec::new();
    let l = io::copy(&mut r, &mut buf).expect("copy must succeed");

    assert_eq!(
        blob_contents.len(),
        l as usize,
        "read bytes must match blob length"
    );

    assert_eq!(blob_contents, buf, "read blob contents must match");
}
