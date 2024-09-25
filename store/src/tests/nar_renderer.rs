use crate::nar::write_nar;
use crate::tests::fixtures::*;
use rstest::*;
use rstest_reuse::*;
use std::io;
use std::sync::Arc;
use tokio::io::sink;
use tvix_castore::blobservice::BlobService;
use tvix_castore::directoryservice::DirectoryService;
use tvix_castore::Node;

/// Make sure the NARRenderer fails if a referred blob doesn't exist.
#[rstest]
#[tokio::test]
async fn single_file_missing_blob(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
) {
    let e = write_nar(
        sink(),
        &CASTORE_NODE_HELLOWORLD,
        // the blobservice is empty intentionally, to provoke the error.
        blob_service,
        directory_service,
    )
    .await
    .expect_err("must fail");

    match e {
        crate::nar::RenderError::NARWriterError(e) => {
            assert_eq!(io::ErrorKind::NotFound, e.kind());
        }
        _ => panic!("unexpected error: {:?}", e),
    }
}

#[apply(castore_fixtures_template)]
#[tokio::test]
async fn seekable(
    #[future] blob_service_with_contents: Arc<dyn BlobService>,
    #[future] directory_service_with_contents: Arc<dyn DirectoryService>,
    #[case] test_input: &Node,
    #[case] test_output: Result<Result<&Vec<u8>, io::ErrorKind>, crate::nar::RenderError>,
) {
    let blob_service = blob_service_with_contents.await;
    let directory_service = directory_service_with_contents.await;

    let mut buf: Vec<u8> = vec![];
    let read_result = write_nar(
        &mut buf,
        test_input,
        // don't put anything in the stores, as we don't actually do any requests.
        blob_service,
        directory_service,
    )
    .await;

    match (read_result, test_output) {
        (Ok(_), Err(_)) => panic!("creating reader should have failed but succeeded"),
        (Ok(_), Ok(Err(_))) => panic!("creating reader should have failed but succeeded"),
        (Err(err), Ok(Ok(_))) => {
            panic!("creating reader should have succeeded but failed: {}", err)
        }
        (Err(reader_err), Err(expected_err)) => {
            assert_eq!(format!("{}", reader_err), format!("{}", expected_err));
        }
        (Err(reader_err), Ok(Err(expected_err))) => {
            let crate::nar::RenderError::NARWriterError(e) = reader_err else {
                panic!("expected nar writer error")
            };
            assert_eq!(e.kind(), expected_err);
        }
        (Ok(_n), Ok(Ok(expected_read_result))) => {
            assert_eq!(buf, expected_read_result.to_vec());
        }
    }
}
