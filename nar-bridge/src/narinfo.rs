use axum::http::StatusCode;
use nix_compat::nixbase32;
use tracing::{instrument, warn, Span};
use tvix_castore::proto::node::Node;

use crate::AppState;

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

    let mut narinfo = path_info.to_narinfo(store_path).ok_or_else(|| {
        warn!(path_info=?path_info, "PathInfo contained no NAR data");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // encode the (unnamed) root node in the NAR url itself.
    let root_node = path_info
        .node
        .as_ref()
        .and_then(|n| n.node.as_ref())
        .expect("root node must not be none")
        .clone()
        .rename("".into());

    let mut buf = Vec::new();
    Node::encode(&root_node, &mut buf);

    let url = format!(
        "nar/tvix-castore/{}?narsize={}",
        data_encoding::BASE64URL_NOPAD.encode(&buf),
        narinfo.nar_size,
    );

    narinfo.url = &url;

    Ok(narinfo.to_string())
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
