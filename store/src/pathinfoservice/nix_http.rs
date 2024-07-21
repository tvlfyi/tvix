use super::PathInfoService;
use crate::{nar::ingest_nar_and_hash, proto::PathInfo};
use futures::{stream::BoxStream, TryStreamExt};
use nix_compat::{
    narinfo::{self, NarInfo},
    nixbase32,
    nixhash::NixHash,
};
use reqwest::StatusCode;
use std::sync::Arc;
use tokio::io::{self, AsyncRead};
use tonic::async_trait;
use tracing::{debug, instrument, warn};
use tvix_castore::composition::{CompositionContext, ServiceBuilder};
use tvix_castore::{
    blobservice::BlobService, directoryservice::DirectoryService, proto as castorepb, Error,
};
use url::Url;

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
/// [PathInfoService::put] is not implemented and returns an error if called.
/// TODO: what about reading from nix-cache-info?
pub struct NixHTTPPathInfoService<BS, DS> {
    base_url: url::Url,
    http_client: reqwest_middleware::ClientWithMiddleware,

    blob_service: BS,
    directory_service: DS,

    /// An optional list of [narinfo::PubKey].
    /// If set, the .narinfo files received need to have correct signature by at least one of these.
    public_keys: Option<Vec<narinfo::VerifyingKey>>,
}

impl<BS, DS> NixHTTPPathInfoService<BS, DS> {
    pub fn new(base_url: url::Url, blob_service: BS, directory_service: DS) -> Self {
        Self {
            base_url,
            http_client: reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
                .with(tvix_tracing::propagate::reqwest::tracing_middleware())
                .build(),
            blob_service,
            directory_service,

            public_keys: None,
        }
    }

    /// Configures [Self] to validate NARInfo fingerprints with the public keys passed.
    pub fn set_public_keys(&mut self, public_keys: Vec<narinfo::VerifyingKey>) {
        self.public_keys = Some(public_keys);
    }
}

#[async_trait]
impl<BS, DS> PathInfoService for NixHTTPPathInfoService<BS, DS>
where
    BS: AsRef<dyn BlobService> + Send + Sync + Clone + 'static,
    DS: AsRef<dyn DirectoryService> + Send + Sync + Clone + 'static,
{
    #[instrument(skip_all, err, fields(path.digest=nixbase32::encode(&digest)))]
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

        // if [self.public_keys] is set, ensure there's at least one valid signature.
        if let Some(public_keys) = &self.public_keys {
            let fingerprint = narinfo.fingerprint();

            if !public_keys.iter().any(|pubkey| {
                narinfo
                    .signatures
                    .iter()
                    .any(|sig| pubkey.verify(&fingerprint, sig))
            }) {
                warn!("no valid signature found");
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "no valid signature found",
                ))?;
            }
        }

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

        // get a reader of the response body.
        let r = tokio_util::io::StreamReader::new(resp.bytes_stream().map_err(|e| {
            let e = e.without_url();
            warn!(e=%e, "failed to get response body");
            io::Error::new(io::ErrorKind::BrokenPipe, e.to_string())
        }));

        // handle decompression, depending on the compression field.
        let mut r: Box<dyn AsyncRead + Send + Unpin> = match narinfo.compression {
            Some("none") => Box::new(r) as Box<dyn AsyncRead + Send + Unpin>,
            Some("bzip2") | None => Box::new(async_compression::tokio::bufread::BzDecoder::new(r))
                as Box<dyn AsyncRead + Send + Unpin>,
            Some("gzip") => Box::new(async_compression::tokio::bufread::GzipDecoder::new(r))
                as Box<dyn AsyncRead + Send + Unpin>,
            Some("xz") => Box::new(async_compression::tokio::bufread::XzDecoder::new(r))
                as Box<dyn AsyncRead + Send + Unpin>,
            Some("zstd") => Box::new(async_compression::tokio::bufread::ZstdDecoder::new(r))
                as Box<dyn AsyncRead + Send + Unpin>,
            Some(comp_str) => {
                return Err(Error::StorageError(format!(
                    "unsupported compression: {comp_str}"
                )));
            }
        };

        let (root_node, nar_hash, nar_size) = ingest_nar_and_hash(
            self.blob_service.clone(),
            self.directory_service.clone(),
            &mut r,
        )
        .await
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // ensure the ingested narhash and narsize do actually match.
        if narinfo.nar_size != nar_size {
            warn!(
                narinfo.nar_size = narinfo.nar_size,
                http.nar_size = nar_size,
                "NarSize mismatch"
            );
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "NarSize mismatch".to_string(),
            ))?;
        }
        if narinfo.nar_hash != nar_hash {
            warn!(
                narinfo.nar_hash = %NixHash::Sha256(narinfo.nar_hash),
                http.nar_hash = %NixHash::Sha256(nar_hash),
                "NarHash mismatch"
            );
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "NarHash mismatch".to_string(),
            ))?;
        }

        Ok(Some(PathInfo {
            node: Some(castorepb::Node {
                // set the name of the root node to the digest-name of the store path.
                node: Some(root_node.rename(narinfo.store_path.to_string().to_owned().into())),
            }),
            references: pathinfo.references,
            narinfo: pathinfo.narinfo,
        }))
    }

    #[instrument(skip_all, fields(path_info=?_path_info))]
    async fn put(&self, _path_info: PathInfo) -> Result<PathInfo, Error> {
        Err(Error::InvalidRequest(
            "put not supported for this backend".to_string(),
        ))
    }

    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
        Box::pin(futures::stream::once(async {
            Err(Error::InvalidRequest(
                "list not supported for this backend".to_string(),
            ))
        }))
    }
}

#[derive(serde::Deserialize)]
pub struct NixHTTPPathInfoServiceConfig {
    base_url: String,
    blob_service: String,
    directory_service: String,
    #[serde(default)]
    /// An optional list of [narinfo::PubKey].
    /// If set, the .narinfo files received need to have correct signature by at least one of these.
    public_keys: Option<Vec<String>>,
}

impl TryFrom<Url> for NixHTTPPathInfoServiceConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(url: Url) -> Result<Self, Self::Error> {
        let mut public_keys: Option<Vec<String>> = None;
        for (_, v) in url
            .query_pairs()
            .into_iter()
            .filter(|(k, _)| k == "trusted-public-keys")
        {
            public_keys
                .get_or_insert(Default::default())
                .extend(v.split_ascii_whitespace().map(ToString::to_string));
        }
        Ok(NixHTTPPathInfoServiceConfig {
            // Stringify the URL and remove the nix+ prefix.
            // We can't use `url.set_scheme(rest)`, as it disallows
            // setting something http(s) that previously wasn't.
            base_url: url.to_string().strip_prefix("nix+").unwrap().to_string(),
            blob_service: "default".to_string(),
            directory_service: "default".to_string(),
            public_keys,
        })
    }
}

#[async_trait]
impl ServiceBuilder for NixHTTPPathInfoServiceConfig {
    type Output = dyn PathInfoService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        context: &CompositionContext,
    ) -> Result<Arc<dyn PathInfoService>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let (blob_service, directory_service) = futures::join!(
            context.resolve(self.blob_service.clone()),
            context.resolve(self.directory_service.clone())
        );
        let mut svc = NixHTTPPathInfoService::new(
            Url::parse(&self.base_url)?,
            blob_service?,
            directory_service?,
        );
        if let Some(public_keys) = &self.public_keys {
            svc.set_public_keys(
                public_keys
                    .iter()
                    .map(|pubkey_str| {
                        narinfo::VerifyingKey::parse(pubkey_str)
                            .map_err(|e| Error::StorageError(format!("invalid public key: {e}")))
                    })
                    .collect::<Result<Vec<_>, Error>>()?,
            );
        }
        Ok(Arc::new(svc))
    }
}
