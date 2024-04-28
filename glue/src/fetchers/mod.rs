use futures::TryStreamExt;
use md5::Md5;
use nix_compat::{
    nixhash::{CAHash, HashAlgo, NixHash},
    store_path::{build_ca_path, BuildStorePathError, StorePathRef},
};
use sha1::Sha1;
use sha2::{digest::Output, Digest, Sha256, Sha512};
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite};
use tokio_util::io::InspectReader;
use tracing::warn;
use tvix_castore::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    proto::{node::Node, FileNode},
};
use tvix_store::{pathinfoservice::PathInfoService, proto::PathInfo};
use url::Url;

use crate::builtins::FetcherError;

mod decompression;
use decompression::DecompressedReader;

/// Representing options for doing a fetch.
#[derive(Clone, Eq, PartialEq)]
pub enum Fetch {
    /// Fetch a literal file from the given URL, with an optional expected
    /// NixHash of it.
    /// TODO: check if this is *always* sha256, and if so, make it [u8; 32].
    URL(Url, Option<NixHash>),

    /// Fetch a tarball from the given URL and unpack.
    /// The file must be a tape archive (.tar), optionally compressed with gzip,
    /// bzip2 or xz.
    /// The top-level path component of the files in the tarball is removed,
    /// so it is best if the tarball contains a single directory at top level.
    /// Optionally, a sha256 digest can be provided to verify the unpacked
    /// contents against.
    Tarball(Url, Option<[u8; 32]>),

    /// TODO
    Git(),
}

// Drops potentially sensitive username and password from a URL.
fn redact_url(url: &Url) -> Url {
    let mut url = url.to_owned();
    if !url.username().is_empty() {
        let _ = url.set_username("redacted");
    }

    if url.password().is_some() {
        let _ = url.set_password(Some("redacted"));
    }

    url
}

impl std::fmt::Debug for Fetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fetch::URL(url, nixhash) => {
                let url = redact_url(url);
                if let Some(nixhash) = nixhash {
                    write!(f, "URL [url: {}, exp_hash: Some({})]", &url, nixhash)
                } else {
                    write!(f, "URL [url: {}, exp_hash: None]", &url)
                }
            }
            Fetch::Tarball(url, exp_digest) => {
                let url = redact_url(url);
                if let Some(exp_digest) = exp_digest {
                    write!(
                        f,
                        "Tarball [url: {}, exp_hash: Some({})]",
                        url,
                        NixHash::Sha256(*exp_digest)
                    )
                } else {
                    write!(f, "Tarball [url: {}, exp_hash: None]", url)
                }
            }
            Fetch::Git() => todo!(),
        }
    }
}

impl Fetch {
    /// If the [Fetch] contains an expected hash upfront, returns the resulting
    /// store path.
    /// This doesn't do any fetching.
    pub fn store_path<'a>(
        &self,
        name: &'a str,
    ) -> Result<Option<StorePathRef<'a>>, BuildStorePathError> {
        let ca_hash = match self {
            Fetch::URL(_, Some(nixhash)) => CAHash::Flat(nixhash.clone()),
            Fetch::Tarball(_, Some(nar_sha256)) => CAHash::Nar(NixHash::Sha256(*nar_sha256)),
            _ => return Ok(None),
        };

        // calculate the store path of this fetch
        build_ca_path(name, &ca_hash, Vec::<String>::new(), false).map(Some)
    }
}

/// Knows how to fetch a given [Fetch].
pub struct Fetcher<BS, DS, PS> {
    http_client: reqwest::Client,
    blob_service: BS,
    directory_service: DS,
    path_info_service: PS,
}

impl<BS, DS, PS> Fetcher<BS, DS, PS> {
    pub fn new(blob_service: BS, directory_service: DS, path_info_service: PS) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            blob_service,
            directory_service,
            path_info_service,
        }
    }

    /// Constructs a HTTP request to the passed URL, and returns a AsyncReadBuf to it.
    /// In case the URI uses the file:// scheme, use tokio::fs to open it.
    async fn download(&self, url: Url) -> Result<Box<dyn AsyncBufRead + Unpin>, FetcherError> {
        match url.scheme() {
            "file" => {
                let f = tokio::fs::File::open(url.to_file_path().map_err(|_| {
                    // "Returns Err if the host is neither empty nor "localhost"
                    // (except on Windows, where file: URLs may have a non-local host)"
                    FetcherError::Io(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "invalid host for file:// scheme",
                    ))
                })?)
                .await?;
                Ok(Box::new(tokio::io::BufReader::new(f)))
            }
            _ => {
                let resp = self.http_client.get(url).send().await?;
                Ok(Box::new(tokio_util::io::StreamReader::new(
                    resp.bytes_stream().map_err(|e| {
                        let e = e.without_url();
                        warn!(%e, "failed to get response body");
                        std::io::Error::new(std::io::ErrorKind::BrokenPipe, e)
                    }),
                )))
            }
        }
    }
}

/// Copies all data from the passed reader to the passed writer.
/// Afterwards, it also returns the resulting [Digest], as well as the number of
/// bytes copied.
/// The exact hash function used is left generic over all [Digest].
async fn hash<D: Digest + std::io::Write>(
    mut r: impl AsyncRead + Unpin,
    mut w: impl AsyncWrite + Unpin,
) -> std::io::Result<(Output<D>, u64)> {
    let mut hasher = D::new();
    let bytes_copied = tokio::io::copy(
        &mut InspectReader::new(&mut r, |d| hasher.write_all(d).unwrap()),
        &mut w,
    )
    .await?;
    Ok((hasher.finalize(), bytes_copied))
}

impl<BS, DS, PS> Fetcher<BS, DS, PS>
where
    BS: AsRef<(dyn BlobService + 'static)> + Clone + Send + Sync + 'static,
    DS: AsRef<(dyn DirectoryService + 'static)>,
    PS: PathInfoService,
{
    /// Ingest the data from a specified [Fetch].
    /// On success, return the root node, a content digest and length.
    /// Returns an error if there was a failure during fetching, or the contents
    /// didn't match the previously communicated hash contained inside the FetchArgs.
    pub async fn ingest(&self, fetch: Fetch) -> Result<(Node, CAHash, u64), FetcherError> {
        match fetch {
            Fetch::URL(url, exp_nixhash) => {
                // Construct a AsyncRead reading from the data as its downloaded.
                let mut r = self.download(url.clone()).await?;

                // Construct a AsyncWrite to write into the BlobService.
                let mut blob_writer = self.blob_service.open_write().await;

                // Copy the contents from the download reader to the blob writer.
                // Calculate the digest of the file received, depending on the
                // communicated expected hash (or sha256 if none provided).
                let (actual_nixhash, blob_size) = match exp_nixhash
                    .as_ref()
                    .map(NixHash::algo)
                    .unwrap_or_else(|| HashAlgo::Sha256)
                {
                    HashAlgo::Sha256 => hash::<Sha256>(&mut r, &mut blob_writer).await.map(
                        |(digest, bytes_written)| (NixHash::Sha256(digest.into()), bytes_written),
                    )?,
                    HashAlgo::Md5 => hash::<Md5>(&mut r, &mut blob_writer).await.map(
                        |(digest, bytes_written)| (NixHash::Md5(digest.into()), bytes_written),
                    )?,
                    HashAlgo::Sha1 => hash::<Sha1>(&mut r, &mut blob_writer).await.map(
                        |(digest, bytes_written)| (NixHash::Sha1(digest.into()), bytes_written),
                    )?,
                    HashAlgo::Sha512 => hash::<Sha512>(&mut r, &mut blob_writer).await.map(
                        |(digest, bytes_written)| {
                            (NixHash::Sha512(Box::new(digest.into())), bytes_written)
                        },
                    )?,
                };

                if let Some(exp_nixhash) = exp_nixhash {
                    if exp_nixhash != actual_nixhash {
                        return Err(FetcherError::HashMismatch {
                            url,
                            wanted: exp_nixhash,
                            got: actual_nixhash,
                        });
                    }
                }

                // Construct and return the FileNode describing the downloaded contents.
                Ok((
                    Node::File(FileNode {
                        name: vec![].into(),
                        digest: blob_writer.close().await?.into(),
                        size: blob_size,
                        executable: false,
                    }),
                    CAHash::Flat(actual_nixhash),
                    blob_size,
                ))
            }
            Fetch::Tarball(url, exp_nar_sha256) => {
                // Construct a AsyncRead reading from the data as its downloaded.
                let r = self.download(url.clone()).await?;

                // Pop compression.
                let r = DecompressedReader::new(r);
                // Open the archive.
                let archive = tokio_tar::Archive::new(r);

                // Ingest the archive, get the root node
                let node = tvix_castore::import::archive::ingest_archive(
                    self.blob_service.clone(),
                    &self.directory_service,
                    archive,
                )
                .await?;

                // If an expected NAR sha256 was provided, compare with the one
                // calculated from our root node.
                // Even if no expected NAR sha256 has been provided, we need
                // the actual one later.
                let (nar_size, actual_nar_sha256) = self
                    .path_info_service
                    .calculate_nar(&node)
                    .await
                    .map_err(|e| {
                        // convert the generic Store error to an IO error.
                        FetcherError::Io(e.into())
                    })?;

                if let Some(exp_nar_sha256) = exp_nar_sha256 {
                    if exp_nar_sha256 != actual_nar_sha256 {
                        return Err(FetcherError::HashMismatch {
                            url,
                            wanted: NixHash::Sha256(exp_nar_sha256),
                            got: NixHash::Sha256(actual_nar_sha256),
                        });
                    }
                }

                Ok((
                    node,
                    CAHash::Nar(NixHash::Sha256(actual_nar_sha256)),
                    nar_size,
                ))
            }
            Fetch::Git() => todo!(),
        }
    }

    /// Ingests the data from a specified [Fetch], persists the returned node
    /// in the PathInfoService, and returns the calculated StorePath, as well as
    /// the root node pointing to the contents.
    /// The root node can be used to descend into the data without doing the
    /// lookup to the PathInfoService again.
    pub async fn ingest_and_persist<'a>(
        &self,
        name: &'a str,
        fetch: Fetch,
    ) -> Result<(StorePathRef<'a>, Node), FetcherError> {
        // Fetch file, return the (unnamed) (File)Node of its contents, ca hash and filesize.
        let (node, ca_hash, size) = self.ingest(fetch).await?;

        // Calculate the store path to return later, which is done with the ca_hash.
        let store_path = build_ca_path(name, &ca_hash, Vec::<String>::new(), false)?;

        // Rename the node name to match the Store Path.
        let node = node.rename(store_path.to_string().into());

        // If the resulting hash is not a CAHash::Nar, we also need to invoke
        // `calculate_nar` to calculate this representation, as it's required in
        // the [PathInfo].
        let (nar_size, nar_sha256) = match &ca_hash {
            CAHash::Flat(_nix_hash) => self
                .path_info_service
                .calculate_nar(&node)
                .await
                .map_err(|e| FetcherError::Io(e.into()))?,
            CAHash::Nar(NixHash::Sha256(nar_sha256)) => (size, *nar_sha256),
            CAHash::Nar(_) => unreachable!("Tvix bug: fetch returned non-sha256 CAHash::Nar"),
            CAHash::Text(_) => unreachable!("Tvix bug: fetch returned CAHash::Text"),
        };

        // Construct the PathInfo and persist it.
        let path_info = PathInfo {
            node: Some(tvix_castore::proto::Node { node: Some(node) }),
            references: vec![],
            narinfo: Some(tvix_store::proto::NarInfo {
                nar_size,
                nar_sha256: nar_sha256.to_vec().into(),
                signatures: vec![],
                reference_names: vec![],
                deriver: None,
                ca: Some(ca_hash.into()),
            }),
        };

        let path_info = self
            .path_info_service
            .put(path_info)
            .await
            .map_err(|e| FetcherError::Io(e.into()))?;

        Ok((store_path, path_info.node.unwrap().node.unwrap()))
    }
}

/// Attempts to mimic `nix::libutil::baseNameOf`
pub(crate) fn url_basename(s: &str) -> &str {
    if s.is_empty() {
        return "";
    }

    let mut last = s.len() - 1;
    if s.chars().nth(last).unwrap() == '/' && last > 0 {
        last -= 1;
    }

    if last == 0 {
        return "";
    }

    let pos = match s[..=last].rfind('/') {
        Some(pos) => {
            if pos == last - 1 {
                0
            } else {
                pos
            }
        }
        None => 0,
    };

    &s[(pos + 1)..=last]
}

#[cfg(test)]
mod tests {
    mod fetch {
        use nix_compat::nixbase32;

        use crate::fetchers::Fetch;

        use super::super::*;

        #[test]
        fn fetchurl_store_path() {
            let url = Url::parse("https://raw.githubusercontent.com/aaptel/notmuch-extract-patch/f732a53e12a7c91a06755ebfab2007adc9b3063b/notmuch-extract-patch").unwrap();
            let exp_nixhash = NixHash::Sha256(
                nixbase32::decode_fixed("0nawkl04sj7psw6ikzay7kydj3dhd0fkwghcsf5rzaw4bmp4kbax")
                    .unwrap(),
            );

            let fetch = Fetch::URL(url, Some(exp_nixhash));
            assert_eq!(
                "06qi00hylriyfm0nl827crgjvbax84mz-notmuch-extract-patch",
                &fetch
                    .store_path("notmuch-extract-patch")
                    .unwrap()
                    .unwrap()
                    .to_string(),
            )
        }

        #[test]
        fn fetch_tarball_store_path() {
            let url = Url::parse("https://github.com/NixOS/nixpkgs/archive/91050ea1e57e50388fa87a3302ba12d188ef723a.tar.gz").unwrap();
            let exp_nixbase32 =
                nixbase32::decode_fixed("1hf6cgaci1n186kkkjq106ryf8mmlq9vnwgfwh625wa8hfgdn4dm")
                    .unwrap();
            let fetch = Fetch::Tarball(url, Some(exp_nixbase32));

            assert_eq!(
                "7adgvk5zdfq4pwrhsm3n9lzypb12gw0g-source",
                &fetch.store_path("source").unwrap().unwrap().to_string(),
            )
        }
    }

    mod url_basename {
        use super::super::*;

        #[test]
        fn empty_path() {
            assert_eq!(url_basename(""), "");
        }

        #[test]
        fn path_on_root() {
            assert_eq!(url_basename("/dir"), "dir");
        }

        #[test]
        fn relative_path() {
            assert_eq!(url_basename("dir/foo"), "foo");
        }

        #[test]
        fn root_with_trailing_slash() {
            assert_eq!(url_basename("/"), "");
        }

        #[test]
        fn trailing_slash() {
            assert_eq!(url_basename("/dir/"), "dir");
        }
    }
}
