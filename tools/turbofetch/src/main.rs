//! turbofetch is a high-performance bulk S3 object aggregator.
//!
//! It operates on two S3 buckets: a source bucket (nix-cache), and a
//! work bucket defined at runtime. The work bucket contains a job file
//! consisting of concatenated 32-character keys, representing narinfo
//! files in the source bucket, without the `.narinfo` suffix or any
//! other separators.
//!
//! Each run of turbofetch processes a half-open range of indices from the
//! job file, and outputs a zstd stream of concatenated objects, without
//! additional separators and in no particular order. These segment files
//! are written into the work bucket, named for the range of indices they
//! cover. `/narinfo.zst/000000000c380d40-000000000c385b60` covers the 20k
//! objects `[0xc380d40, 0xc385b60) = [205000000, 205020000)`. Empirically,
//! segment files of 20k objects achieve a compression ratio of 4.7x.
//!
//! Reassembly is left to narinfo2parquet, which interprets StorePath lines.
//!
//! TODO(edef): any retries/error handling whatsoever
//! Currently, it fails an entire range if anything goes wrong, and doesn't
//! write any output.

use bytes::Bytes;
use futures::{stream::FuturesUnordered, Stream, TryStreamExt};
use rusoto_core::ByteStream;
use rusoto_s3::{GetObjectRequest, PutObjectRequest, S3Client, S3};
use serde::Deserialize;
use std::{io::Write, mem, ops::Range, ptr};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

/// Fetch a group of keys, streaming concatenated chunks as they arrive from S3.
/// `keys` must be a slice from the job file. Any network error at all fails the
/// entire batch, and there is no rate limiting.
fn fetch(keys: &[[u8; 32]]) -> impl Stream<Item = io::Result<Bytes>> {
    // S3 supports only HTTP/1.1, but we can ease the pain somewhat by using
    // HTTP pipelining. It terminates the TCP connection after receiving 100
    // requests, so we chunk the keys up accordingly, and make one connection
    // for each chunk.
    keys.chunks(100)
        .map(|chunk| {
            const PREFIX: &[u8] = b"GET /nix-cache/";
            const SUFFIX: &[u8] = b".narinfo HTTP/1.1\nHost: s3.amazonaws.com\n\n";
            const LENGTH: usize = PREFIX.len() + 32 + SUFFIX.len();

            let mut request = Vec::with_capacity(LENGTH * 100);
            for key in chunk {
                request.extend_from_slice(PREFIX);
                request.extend_from_slice(key);
                request.extend_from_slice(SUFFIX);
            }

            (request, chunk.len())
        })
        .map(|(request, n)| async move {
            let (mut read, mut write) = TcpStream::connect("s3.amazonaws.com:80")
                .await?
                .into_split();

            let _handle = tokio::spawn(async move {
                let request = request;
                write.write_all(&request).await
            });

            let mut buffer = turbofetch::Buffer::new(512 * 1024);
            let mut bodies = vec![];

            for _ in 0..n {
                let body = turbofetch::parse_response(&mut read, &mut buffer).await?;
                bodies.extend_from_slice(body);
            }

            Ok::<_, io::Error>(Bytes::from(bodies))
        })
        .collect::<FuturesUnordered<_>>()
}

/// Retrieve a range of keys from the job file.
async fn get_range(
    s3: &'static S3Client,
    bucket: String,
    key: String,
    range: Range<u64>,
) -> io::Result<Box<[[u8; 32]]>> {
    let resp = s3
        .get_object(GetObjectRequest {
            bucket,
            key,
            range: Some(format!("bytes={}-{}", range.start * 32, range.end * 32 - 1)),
            ..GetObjectRequest::default()
        })
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    let mut body = vec![];
    resp.body
        .ok_or(io::ErrorKind::InvalidData)?
        .into_async_read()
        .read_to_end(&mut body)
        .await?;

    let body = exact_chunks(body.into_boxed_slice()).ok_or(io::ErrorKind::InvalidData)?;

    Ok(body)
}

fn exact_chunks(mut buf: Box<[u8]>) -> Option<Box<[[u8; 32]]>> {
    // SAFETY: We ensure that `buf.len()` is a multiple of 32, and there are no alignment requirements.
    unsafe {
        let ptr = buf.as_mut_ptr();
        let len = buf.len();

        if len % 32 != 0 {
            return None;
        }

        let ptr = ptr as *mut [u8; 32];
        let len = len / 32;
        mem::forget(buf);

        Some(Box::from_raw(ptr::slice_from_raw_parts_mut(ptr, len)))
    }
}

// TODO(edef): factor this out into a separate entry point
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), lambda_runtime::Error> {
    let s3 = S3Client::new(rusoto_core::Region::UsEast1);
    let s3 = &*Box::leak(Box::new(s3));

    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        // this needs to be set to remove duplicated information in the log.
        .with_current_span(false)
        // this needs to be set to false, otherwise ANSI color codes will
        // show up in a confusing manner in CloudWatch logs.
        .with_ansi(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        // remove the name of the function from every log entry
        .with_target(false)
        .init();

    lambda_runtime::run(lambda_runtime::service_fn(|event| func(s3, event))).await
}

/// Lambda request body
#[derive(Debug, Deserialize)]
struct Params {
    work_bucket: String,
    job_file: String,
    start: u64,
    end: u64,
}

#[tracing::instrument(skip(s3, event), fields(req_id = %event.context.request_id))]
async fn func(
    s3: &'static S3Client,
    event: lambda_runtime::LambdaEvent<
        aws_lambda_events::lambda_function_urls::LambdaFunctionUrlRequest,
    >,
) -> Result<&'static str, lambda_runtime::Error> {
    let mut params = event.payload.body.ok_or("no body")?;

    if event.payload.is_base64_encoded {
        params = String::from_utf8(data_encoding::BASE64.decode(params.as_bytes())?)?;
    }

    let params: Params = serde_json::from_str(&params)?;

    if params.start >= params.end {
        return Err("nope".into());
    }

    let keys = get_range(
        s3,
        params.work_bucket.clone(),
        params.job_file.to_owned(),
        params.start..params.end,
    )
    .await?;

    let zchunks = fetch(&keys)
        .try_fold(
            Box::new(zstd::Encoder::new(vec![], zstd::DEFAULT_COMPRESSION_LEVEL).unwrap()),
            |mut w, buf| {
                w.write_all(&buf).unwrap();
                async { Ok(w) }
            },
        )
        .await?;

    let zchunks = to_byte_stream(zchunks.finish().unwrap());

    tracing::info!("we got to put_object");

    s3.put_object(PutObjectRequest {
        bucket: params.work_bucket,
        key: format!("narinfo.zst/{:016x}-{:016x}", params.start, params.end),
        body: Some(zchunks),
        ..Default::default()
    })
    .await
    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

    tracing::info!("… and it worked!");

    Ok("OK")
}

fn to_byte_stream(buffer: Vec<u8>) -> ByteStream {
    let size_hint = buffer.len();
    ByteStream::new_with_size(
        futures::stream::once(async { Ok(buffer.into()) }),
        size_hint,
    )
}
