use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::Response;
use axum::{body::Body, response::IntoResponse};
use axum_extra::{headers::Range, TypedHeader};
use axum_range::{KnownSize, Ranged};
use bytes::Bytes;
use data_encoding::BASE64URL_NOPAD;
use futures::TryStreamExt;
use nix_compat::{nix_http, nixbase32};
use serde::Deserialize;
use std::io;
use tokio_util::io::ReaderStream;
use tracing::{instrument, warn, Span};
use tvix_store::nar::ingest_nar_and_hash;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub(crate) struct GetNARParams {
    #[serde(rename = "narsize")]
    nar_size: u64,
}

#[instrument(skip(blob_service, directory_service))]
pub async fn get_head(
    method: axum::http::Method,
    ranges: Option<TypedHeader<Range>>,
    axum::extract::Path(root_node_enc): axum::extract::Path<String>,
    axum::extract::Query(GetNARParams { nar_size }): Query<GetNARParams>,
    axum::extract::State(AppState {
        blob_service,
        directory_service,
        ..
    }): axum::extract::State<AppState>,
) -> Result<impl axum::response::IntoResponse, StatusCode> {
    use prost::Message;
    // b64decode the root node passed *by the user*
    let root_node_proto = BASE64URL_NOPAD
        .decode(root_node_enc.as_bytes())
        .map_err(|e| {
            warn!(err=%e, "unable to decode root node b64");
            StatusCode::NOT_FOUND
        })?;

    // check the proto size to be somewhat reasonable before parsing it.
    if root_node_proto.len() > 4096 {
        warn!("rejected too large root node");
        return Err(StatusCode::BAD_REQUEST);
    }

    // parse the proto
    let root_node: tvix_castore::proto::Node = Message::decode(Bytes::from(root_node_proto))
        .map_err(|e| {
            warn!(err=%e, "unable to decode root node proto");
            StatusCode::NOT_FOUND
        })?;

    let root_node = root_node.try_into_anonymous_node().map_err(|e| {
        warn!(err=%e, "root node validation failed");
        StatusCode::BAD_REQUEST
    })?;

    Ok((
        // headers
        [
            ("cache-control", "max-age=31536000, immutable"),
            ("content-type", nix_http::MIME_TYPE_NAR),
        ],
        if method == axum::http::Method::HEAD {
            // If this is a HEAD request, construct a response returning back the
            // user-provided content-length, but don't actually talk to castore.
            Response::builder()
                .header("content-length", nar_size)
                .body(Body::empty())
                .unwrap()
        } else if let Some(TypedHeader(ranges)) = ranges {
            // If this is a range request, construct a seekable NAR reader.
            let r =
                tvix_store::nar::seekable::Reader::new(root_node, blob_service, directory_service)
                    .await
                    .map_err(|e| {
                        warn!(err=%e, "failed to construct seekable nar reader");
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;

            // ensure the user-supplied nar size was correct, no point returning data otherwise.
            if r.stream_len() != nar_size {
                warn!(
                    actual_nar_size = r.stream_len(),
                    supplied_nar_size = nar_size,
                    "wrong nar size supplied"
                );
                return Err(StatusCode::BAD_REQUEST);
            }
            Ranged::new(Some(ranges), KnownSize::sized(r, nar_size)).into_response()
        } else {
            // use the non-seekable codepath if there's no range(s) requested,
            // as it uses less memory.
            let (w, r) = tokio::io::duplex(1024 * 8);

            // spawn a task rendering the NAR to the client.
            tokio::spawn(async move {
                if let Err(e) =
                    tvix_store::nar::write_nar(w, &root_node, blob_service, directory_service).await
                {
                    warn!(err=%e, "failed to write out NAR");
                }
            });

            Response::builder()
                .header("content-length", nar_size)
                .body(Body::from_stream(ReaderStream::new(r)))
                .unwrap()
        },
    ))
}

#[instrument(skip(blob_service, directory_service, request))]
pub async fn put(
    axum::extract::Path(nar_str): axum::extract::Path<String>,
    axum::extract::State(AppState {
        blob_service,
        directory_service,
        root_nodes,
        ..
    }): axum::extract::State<AppState>,
    request: axum::extract::Request,
) -> Result<&'static str, StatusCode> {
    let (nar_hash_expected, compression_suffix) =
        nix_http::parse_nar_str(&nar_str).ok_or(StatusCode::UNAUTHORIZED)?;

    // No paths with compression suffix are supported.
    if !compression_suffix.is_empty() {
        warn!(%compression_suffix, "invalid compression suffix requested");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let s = request.into_body().into_data_stream();

    let mut r = tokio_util::io::StreamReader::new(s.map_err(|e| {
        warn!(err=%e, "failed to read request body");
        io::Error::new(io::ErrorKind::BrokenPipe, e.to_string())
    }));

    // ingest the NAR
    let (root_node, nar_hash_actual, nar_size) =
        ingest_nar_and_hash(blob_service.clone(), directory_service.clone(), &mut r)
            .await
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
            .map_err(|e| {
                warn!(err=%e, "failed to ingest nar");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

    let s = Span::current();
    s.record("nar_hash.expected", nixbase32::encode(&nar_hash_expected));
    s.record("nar_size", nar_size);

    if nar_hash_expected != nar_hash_actual {
        warn!(
            nar_hash.expected = nixbase32::encode(&nar_hash_expected),
            nar_hash.actual = nixbase32::encode(&nar_hash_actual),
            "nar hash mismatch"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // store mapping of narhash to root node into root_nodes.
    // we need it later to populate the root node when accepting the PathInfo.
    root_nodes.write().put(nar_hash_actual, root_node);

    Ok("")
}

// FUTUREWORK: maybe head by narhash. Though not too critical, as we do
// implement HEAD for .narinfo.
