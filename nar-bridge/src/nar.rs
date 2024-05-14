use axum::body::Body;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::Response;
use bytes::Bytes;
use data_encoding::BASE64URL_NOPAD;
use serde::Deserialize;
use tokio_util::io::ReaderStream;
use tracing::{instrument, warn};

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
    let root_node: tvix_castore::proto::Node = Message::decode(Bytes::from(root_node_enc))
        .map_err(|e| {
            warn!(err=%e, "unable to decode root node proto");
            StatusCode::NOT_FOUND
        })?;

    // validate it.
    let root_node = root_node
        .validate()
        .map_err(|e| {
            warn!(err=%e, "root node validation failed");
            StatusCode::BAD_REQUEST
        })?
        .to_owned();

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
