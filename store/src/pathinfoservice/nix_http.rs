use std::{
    io::{self, BufRead},
    pin::Pin,
    sync::Arc,
};

use data_encoding::BASE64;
use futures::{Stream, TryStreamExt};
use nix_compat::{narinfo::NarInfo, nixbase32};
use reqwest::StatusCode;
use tonic::async_trait;
use tracing::{debug, instrument, warn};
use tvix_castore::{
    blobservice::BlobService, directoryservice::DirectoryService, proto as castorepb, Error,
};

use crate::proto::PathInfo;

use super::PathInfoService;

/// NixHTTPPathInfoService acts as a bridge in between the Nix HTTP Binary cache
/// protocol provided by Nix binary caches such as cache.nixos.org, and the Tvix
/// Store Model.
/// It implements the [PathInfoService] trait in an interesting way:
/// Every [PathInfoService::get] fetches the .narinfo and referred NAR file,
/// inserting components into a [BlobService] and [DirectoryService], then
/// returning a [PathInfo] struct with the root.
///
/// Due to this being quite a costly operation, clients are expected to layer
/// this service with store composition, so they're only ingested once.
///
/// The client is expected to be (indirectly) using the same [BlobService] and
/// [DirectoryService], so able to fetch referred Directories and Blobs.
/// [PathInfoService::put] and [PathInfoService::nar] are not implemented and
/// return an error if called.
/// TODO: what about reading from nix-cache-info?
pub struct NixHTTPPathInfoService {
    base_url: url::Url,
    http_client: reqwest::Client,

    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
}

impl NixHTTPPathInfoService {
    pub fn new(
        base_url: url::Url,
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
    ) -> Self {
        Self {
            base_url,
            http_client: reqwest::Client::new(),
            blob_service,
            directory_service,
        }
    }
}

#[async_trait]
impl PathInfoService for NixHTTPPathInfoService {
    #[instrument(skip_all, err, fields(path.digest=BASE64.encode(&digest)))]
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        let narinfo_url = self
            .base_url
            .join(&format!("{}.narinfo", nixbase32::encode(&digest)))
            .map_err(|e| {
                warn!(e = %e, "unable to join URL");
                io::Error::new(io::ErrorKind::InvalidInput, "unable to join url")
            })?;

        debug!(narinfo_url= %narinfo_url, "constructed NARInfo url");

        let resp = self
            .http_client
            .get(narinfo_url)
            .send()
            .await
            .map_err(|e| {
                warn!(e=%e,"unable to send NARInfo request");
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unable to send NARInfo request",
                )
            })?;

        // In the case of a 404, return a NotFound.
        // We also return a NotFound in case of a 403 - this is to match the behaviour as Nix,
        // when querying nix-cache.s3.amazonaws.com directly, rather than cache.nixos.org.
        if resp.status() == StatusCode::NOT_FOUND || resp.status() == StatusCode::FORBIDDEN {
            return Ok(None);
        }

        let narinfo_str = resp.text().await.map_err(|e| {
            warn!(e=%e,"unable to decode response as string");
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to decode response as string",
            )
        })?;

        // parse the received narinfo
        let narinfo = NarInfo::parse(&narinfo_str).map_err(|e| {
            warn!(e=%e,"unable to parse response as NarInfo");
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to parse response as NarInfo",
            )
        })?;

        // Convert to a (sparse) PathInfo. We still need to populate the node field,
        // and for this we need to download the NAR file.
        // FUTUREWORK: Keep some database around mapping from narsha256 to
        // (unnamed) rootnode, so we can use that (and the name from the
        // StorePath) and avoid downloading the same NAR a second time.
        let pathinfo: PathInfo = (&narinfo).into();

        // create a request for the NAR file itself.
        let nar_url = self.base_url.join(narinfo.url).map_err(|e| {
            warn!(e = %e, "unable to join URL");
            io::Error::new(io::ErrorKind::InvalidInput, "unable to join url")
        })?;
        debug!(nar_url= %nar_url, "constructed NAR url");

        let resp = self
            .http_client
            .get(nar_url.clone())
            .send()
            .await
            .map_err(|e| {
                warn!(e=%e,"unable to send NAR request");
                io::Error::new(io::ErrorKind::InvalidInput, "unable to send NAR request")
            })?;

        // if the request is not successful, return an error.
        if !resp.status().is_success() {
            return Err(Error::StorageError(format!(
                "unable to retrieve NAR at {}, status {}",
                nar_url,
                resp.status()
            )));
        }

        // get an AsyncRead of the response body.
        let async_r = tokio_util::io::StreamReader::new(resp.bytes_stream().map_err(|e| {
            let e = e.without_url();
            warn!(e=%e, "failed to get response body");
            io::Error::new(io::ErrorKind::BrokenPipe, e.to_string())
        }));
        let sync_r = std::io::BufReader::new(tokio_util::io::SyncIoBridge::new(async_r));

        // handle decompression, by wrapping the reader.
        let mut sync_r: Box<dyn BufRead + Send> = match narinfo.compression {
            Some("none") => Box::new(sync_r),
            Some("xz") => Box::new(std::io::BufReader::new(xz2::read::XzDecoder::new(sync_r))),
            Some(comp) => {
                return Err(Error::InvalidRequest(
                    format!("unsupported compression: {}", comp).to_string(),
                ))
            }
            None => {
                return Err(Error::InvalidRequest(
                    "unsupported compression: bzip2".to_string(),
                ))
            }
        };

        let res = tokio::task::spawn_blocking({
            let blob_service = self.blob_service.clone();
            let directory_service = self.directory_service.clone();
            move || crate::nar::read_nar(&mut sync_r, blob_service, directory_service)
        })
        .await
        .unwrap();

        match res {
            Ok(root_node) => Ok(Some(PathInfo {
                node: Some(castorepb::Node {
                    // set the name of the root node to the digest-name of the store path.
                    node: Some(root_node.rename(narinfo.store_path.to_string().to_owned().into())),
                }),
                references: pathinfo.references,
                narinfo: pathinfo.narinfo,
            })),
            Err(e) => Err(e.into()),
        }
    }

    #[instrument(skip_all, fields(path_info=?_path_info))]
    async fn put(&self, _path_info: PathInfo) -> Result<PathInfo, Error> {
        Err(Error::InvalidRequest(
            "put not supported for this backend".to_string(),
        ))
    }

    #[instrument(skip_all, fields(root_node=?root_node))]
    async fn calculate_nar(
        &self,
        root_node: &castorepb::node::Node,
    ) -> Result<(u64, [u8; 32]), Error> {
        Err(Error::InvalidRequest(
            "calculate_nar not supported for this backend".to_string(),
        ))
    }

    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<PathInfo, Error>> + Send>> {
        Box::pin(futures::stream::once(async {
            Err(Error::InvalidRequest(
                "list not supported for this backend".to_string(),
            ))
        }))
    }
}
