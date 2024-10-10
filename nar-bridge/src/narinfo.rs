use axum::{http::StatusCode, response::IntoResponse};
use bytes::Bytes;
use nix_compat::{
    narinfo::{NarInfo, Signature},
    nix_http, nixbase32,
    store_path::StorePath,
};
use prost::Message;
use tracing::{instrument, warn, Span};
use tvix_castore::proto::{self as castorepb};
use tvix_store::pathinfoservice::PathInfo;

use crate::AppState;

/// The size limit for NARInfo uploads nar-bridge receives
const NARINFO_LIMIT: usize = 2 * 1024 * 1024;

#[instrument(skip(path_info_service))]
pub async fn head(
    axum::extract::Path(narinfo_str): axum::extract::Path<String>,
    axum::extract::State(AppState {
        path_info_service, ..
    }): axum::extract::State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let digest = nix_http::parse_narinfo_str(&narinfo_str).ok_or(StatusCode::NOT_FOUND)?;
    Span::current().record("path_info.digest", &narinfo_str[0..32]);

    if path_info_service
        .get(digest)
        .await
        .map_err(|e| {
            warn!(err=%e, "failed to get PathInfo");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .is_some()
    {
        Ok(([("content-type", nix_http::MIME_TYPE_NARINFO)], ""))
    } else {
        warn!("PathInfo not found");
        Err(StatusCode::NOT_FOUND)
    }
}

#[instrument(skip(path_info_service))]
pub async fn get(
    axum::extract::Path(narinfo_str): axum::extract::Path<String>,
    axum::extract::State(AppState {
        path_info_service, ..
    }): axum::extract::State<AppState>,
) -> Result<impl IntoResponse, StatusCode> {
    let digest = nix_http::parse_narinfo_str(&narinfo_str).ok_or(StatusCode::NOT_FOUND)?;
    Span::current().record("path_info.digest", &narinfo_str[0..32]);

    // fetch the PathInfo
    let path_info = path_info_service
        .get(digest)
        .await
        .map_err(|e| {
            warn!(err=%e, "failed to get PathInfo");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let url = format!(
        "nar/tvix-castore/{}?narsize={}",
        data_encoding::BASE64URL_NOPAD.encode(
            &castorepb::Node::from_name_and_node("".into(), path_info.node.clone()).encode_to_vec()
        ),
        path_info.nar_size,
    );

    let mut narinfo = path_info.to_narinfo();
    narinfo.url = &url;

    Ok((
        [("content-type", nix_http::MIME_TYPE_NARINFO)],
        narinfo.to_string(),
    ))
}

#[instrument(skip(path_info_service, root_nodes, request))]
pub async fn put(
    axum::extract::Path(narinfo_str): axum::extract::Path<String>,
    axum::extract::State(AppState {
        path_info_service,
        root_nodes,
        ..
    }): axum::extract::State<AppState>,
    request: axum::extract::Request,
) -> Result<&'static str, StatusCode> {
    let _narinfo_digest = nix_http::parse_narinfo_str(&narinfo_str).ok_or(StatusCode::UNAUTHORIZED);
    Span::current().record("path_info.digest", &narinfo_str[0..32]);

    let narinfo_bytes: Bytes = axum::body::to_bytes(request.into_body(), NARINFO_LIMIT)
        .await
        .map_err(|e| {
            warn!(err=%e, "unable to fetch body");
            StatusCode::BAD_REQUEST
        })?;

    // Parse the narinfo from the body.
    let narinfo_str = std::str::from_utf8(narinfo_bytes.as_ref()).map_err(|e| {
        warn!(err=%e, "unable decode body as string");
        StatusCode::BAD_REQUEST
    })?;

    let narinfo = NarInfo::parse(narinfo_str).map_err(|e| {
        warn!(err=%e, "unable to parse narinfo");
        StatusCode::BAD_REQUEST
    })?;

    // Extract the NARHash from the PathInfo.
    Span::current().record("path_info.nar_info", nixbase32::encode(&narinfo.nar_hash));

    // Lookup root node with peek, as we don't want to update the LRU list.
    // We need to be careful to not hold the RwLock across the await point.
    let maybe_root_node: Option<tvix_castore::Node> =
        root_nodes.read().peek(&narinfo.nar_hash).cloned();

    match maybe_root_node {
        Some(root_node) => {
            // Persist the PathInfo.
            path_info_service
                .put(PathInfo {
                    store_path: narinfo.store_path.to_owned(),
                    node: root_node,
                    references: narinfo.references.iter().map(StorePath::to_owned).collect(),
                    nar_sha256: narinfo.nar_hash,
                    nar_size: narinfo.nar_size,
                    signatures: narinfo
                        .signatures
                        .into_iter()
                        .map(|s| {
                            Signature::<String>::new(s.name().to_string(), s.bytes().to_owned())
                        })
                        .collect(),
                    deriver: narinfo.deriver.as_ref().map(StorePath::to_owned),
                    ca: narinfo.ca,
                })
                .await
                .map_err(|e| {
                    warn!(err=%e, "failed to persist the PathInfo");
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            Ok("")
        }
        None => {
            warn!("received narinfo with unknown NARHash");
            Err(StatusCode::BAD_REQUEST)
        }
    }
}
