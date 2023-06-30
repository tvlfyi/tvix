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

/// Put a blob in the store, and seek inside it a bit.
#[test_case(gen_memory_blob_service(); "memory")]
#[test_case(gen_sled_blob_service(); "sled")]
fn put_seek(blob_service: impl BlobService) {
    let mut w = blob_service.open_write();

    io::copy(&mut io::Cursor::new(&fixtures::BLOB_B.to_vec()), &mut w).expect("copy must succeed");
    w.close().expect("close must succeed");

    // open a blob for reading
    let mut r = blob_service
        .open_read(&fixtures::BLOB_B_DIGEST)
        .expect("open_read must succeed")
        .expect("must be some");

    let mut pos: u64 = 0;

    // read the first 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).expect("must succeed");

        assert_eq!(
            &fixtures::BLOB_B[pos as usize..pos as usize + buf.len()],
            buf,
            "expected first 10 bytes to match"
        );

        pos += buf.len() as u64;
    }
    // seek by 0 bytes, using SeekFrom::Start.
    let p = r
        .seek(io::SeekFrom::Start(pos as u64))
        .expect("must not fail");
    assert_eq!(pos, p);

    // read the next 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).expect("must succeed");

        assert_eq!(
            &fixtures::BLOB_B[pos as usize..pos as usize + buf.len()],
            buf,
            "expected data to match"
        );

        pos += buf.len() as u64;
    }

    // seek by 5 bytes, using SeekFrom::Start.
    let p = r
        .seek(io::SeekFrom::Start(pos as u64 + 5))
        .expect("must not fail");
    pos += 5;
    assert_eq!(pos, p);

    // read the next 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).expect("must succeed");

        assert_eq!(
            &fixtures::BLOB_B[pos as usize..pos as usize + buf.len()],
            buf,
            "expected data to match"
        );

        pos += buf.len() as u64;
    }

    // seek by 12345 bytes, using SeekFrom::
    let p = r.seek(io::SeekFrom::Current(12345)).expect("must not fail");
    pos += 12345;
    assert_eq!(pos, p);

    // read the next 10 bytes, they must match the data in the fixture.
    {
        let mut buf = [0; 10];
        r.read_exact(&mut buf).expect("must succeed");

        assert_eq!(
            &fixtures::BLOB_B[pos as usize..pos as usize + buf.len()],
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
        .seek(io::SeekFrom::Start(fixtures::BLOB_B.len() as u64))
        .expect("must not fail");
    pos = fixtures::BLOB_B.len() as u64;
    assert_eq!(pos, p);

    {
        // but it returns no more data.
        let mut buf: Vec<u8> = Vec::new();
        r.read_to_end(&mut buf).expect("must not fail");
        assert!(buf.is_empty(), "expected no more data to be read");
    }

    // seeking past the end…
    match r.seek(io::SeekFrom::Start(fixtures::BLOB_B.len() as u64 + 1)) {
        // should either be ok, but then return 0 bytes.
        // this matches the behaviour or a Cursor<Vec<u8>>.
        Ok(_pos) => {
            let mut buf: Vec<u8> = Vec::new();
            r.read_to_end(&mut buf).expect("must not fail");
            assert!(buf.is_empty(), "expected no more data to be read");
        }
        // or not be okay.
        Err(_) => {}
    }

    // TODO: this is only broken for the gRPC version
    // We expect seeking backwards or relative to the end to fail.
    // r.seek(io::SeekFrom::Current(-1))
    //     .expect_err("SeekFrom::Current(-1) expected to fail");

    // r.seek(io::SeekFrom::Start(pos - 1))
    //     .expect_err("SeekFrom::Start(pos-1) expected to fail");

    // r.seek(io::SeekFrom::End(0))
    //     .expect_err("SeekFrom::End(_) expected to fail");
}
