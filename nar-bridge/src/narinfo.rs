use axum::http::StatusCode;
use bytes::Bytes;
use nix_compat::{narinfo::NarInfo, nixbase32};
use tracing::{instrument, warn, Span};
use tvix_castore::proto::{self as castorepb, node::Node};
use tvix_store::proto::PathInfo;

use crate::AppState;

/// The size limit for NARInfo uploads nar-bridge receives
const NARINFO_LIMIT: usize = 2 * 1024 * 1024;

#[instrument(skip(path_info_service))]
pub async fn head(
    axum::extract::Path(narinfo_str): axum::extract::Path<String>,
    axum::extract::State(AppState {
        path_info_service, ..
    }): axum::extract::State<AppState>,
) -> Result<&'static str, StatusCode> {
    let digest = parse_narinfo_str(&narinfo_str)?;
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
        Ok("")
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
) -> Result<String, StatusCode> {
    let digest = parse_narinfo_str(&narinfo_str)?;
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

    let store_path = path_info.validate().map_err(|e| {
        warn!(err=%e, "invalid PathInfo");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut narinfo = path_info.to_narinfo(store_path.as_ref()).ok_or_else(|| {
        warn!(path_info=?path_info, "PathInfo contained no NAR data");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // encode the (unnamed) root node in the NAR url itself.
    let root_node = tvix_castore::directoryservice::Node::try_from(
        path_info.node.as_ref().expect("root node must not be none"),
    )
    .unwrap() // PathInfo is validated
    .rename("".into());

    let mut buf = Vec::new();
    Node::encode(&(&root_node).into(), &mut buf);

    let url = format!(
        "nar/tvix-castore/{}?narsize={}",
        data_encoding::BASE64URL_NOPAD.encode(&buf),
        narinfo.nar_size,
    );

    narinfo.url = &url;

    Ok(narinfo.to_string())
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
    let _narinfo_digest = parse_narinfo_str(&narinfo_str)?;
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

    // populate the pathinfo.
    let mut pathinfo = PathInfo::from(&narinfo);

    // Lookup root node with peek, as we don't want to update the LRU list.
    // We need to be careful to not hold the RwLock across the await point.
    let maybe_root_node: Option<tvix_castore::directoryservice::Node> = root_nodes
        .read()
        .peek(&narinfo.nar_hash)
        .and_then(|v| v.try_into().ok());

    match maybe_root_node {
        Some(root_node) => {
            // Set the root node from the lookup.
            // We need to rename the node to the narinfo storepath basename, as
            // that's where it's stored in PathInfo.
            pathinfo.node = Some(castorepb::Node {
                node: Some((&root_node.rename(narinfo.store_path.to_string().into())).into()),
            });

            // Persist the PathInfo.
            path_info_service.put(pathinfo).await.map_err(|e| {
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

/// Parses a `3mzh8lvgbynm9daj7c82k2sfsfhrsfsy.narinfo` string and returns the
/// nixbase32-decoded digest.
fn parse_narinfo_str(s: &str) -> Result<[u8; 20], StatusCode> {
    if !s.is_char_boundary(32) {
        warn!("invalid string, no char boundary at 32");
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(match s.split_at(32) {
        (hash_str, ".narinfo") => {
            // we know this is 32 bytes
            let hash_str_fixed: [u8; 32] = hash_str.as_bytes().try_into().unwrap();
            nixbase32::decode_fixed(hash_str_fixed).map_err(|e| {
                warn!(err=%e, "invalid digest");
                StatusCode::NOT_FOUND
            })?
        }
        _ => {
            warn!("invalid string");
            return Err(StatusCode::NOT_FOUND);
        }
    })
}

#[cfg(test)]
mod test {
    use super::parse_narinfo_str;
    use hex_literal::hex;

    #[test]
    fn success() {
        assert_eq!(
            hex!("8a12321522fd91efbd60ebb2481af88580f61600"),
            parse_narinfo_str("00bgd045z0d4icpbc2yyz4gx48ak44la.narinfo").unwrap()
        );
    }

    #[test]
    fn failure() {
        assert!(parse_narinfo_str("00bgd045z0d4icpbc2yyz4gx48ak44la").is_err());
        assert!(parse_narinfo_str("/00bgd045z0d4icpbc2yyz4gx48ak44la").is_err());
        assert!(parse_narinfo_str("000000").is_err());
        assert!(parse_narinfo_str("00bgd045z0d4icpbc2yyz4gx48ak44l🦊.narinfo").is_err());
    }
}
