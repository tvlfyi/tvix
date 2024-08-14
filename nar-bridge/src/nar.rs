use axum::body::Body;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::Response;
use bytes::Bytes;
use data_encoding::BASE64URL_NOPAD;
use futures::TryStreamExt;
use nix_compat::nixbase32;
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
pub async fn get(
    axum::extract::Path(root_node_enc): axum::extract::Path<String>,
    axum::extract::Query(GetNARParams { nar_size }): Query<GetNARParams>,
    axum::extract::State(AppState {
        blob_service,
        directory_service,
        ..
    }): axum::extract::State<AppState>,
) -> Result<Response, StatusCode> {
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

    let (root_name, root_node) = root_node.into_name_and_node().map_err(|e| {
        warn!(err=%e, "root node validation failed");
        StatusCode::BAD_REQUEST
    })?;

    if !root_name.is_empty() {
        warn!("root node has name, which it shouldn't");
        return Err(StatusCode::BAD_REQUEST);
    }

    let (w, r) = tokio::io::duplex(1024 * 8);

    // spawn a task rendering the NAR to the client
    tokio::spawn(async move {
        if let Err(e) =
            tvix_store::nar::write_nar(w, &root_node, blob_service, directory_service).await
        {
            warn!(err=%e, "failed to write out NAR");
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("cache-control", "max-age=31536000, immutable")
        .header("content-length", nar_size)
        .body(Body::from_stream(ReaderStream::new(r)))
        .unwrap())
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
    let nar_hash_expected = parse_nar_str(&nar_str)?;

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

/// Parses a `14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0k.nar`
/// string and returns the nixbase32-decoded digest.
/// No compression is supported.
fn parse_nar_str(s: &str) -> Result<[u8; 32], StatusCode> {
    if !s.is_char_boundary(52) {
        warn!("invalid string, no char boundary at 32");
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(match s.split_at(52) {
        (hash_str, ".nar") => {
            // we know this is 52 bytes
            let hash_str_fixed: [u8; 52] = hash_str.as_bytes().try_into().unwrap();
            nixbase32::decode_fixed(hash_str_fixed).map_err(|e| {
                warn!(err=%e, "invalid digest");
                StatusCode::NOT_FOUND
            })?
        }
        _ => {
            warn!("invalid string");
            return Err(StatusCode::BAD_REQUEST);
        }
    })
}

#[cfg(test)]
mod test {
    use super::parse_nar_str;
    use hex_literal::hex;

    #[test]
    fn success() {
        assert_eq!(
            hex!("13a8cf7ca57f68a9f1752acee36a72a55187d3a954443c112818926f26109d91"),
            parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0k.nar").unwrap()
        )
    }

    #[test]
    fn failure() {
        assert!(
            parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0k.nar.x").is_err()
        );
        assert!(
            parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0k.nar.xz").is_err()
        );
        assert!(parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0").is_err());
        assert!(parse_nar_str("14cx20k6z4hq508kqi2lm79qfld5f9mf7kiafpqsjs3zlmycza0🦊.nar").is_err())
    }
}
