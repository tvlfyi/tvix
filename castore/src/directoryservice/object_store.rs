use std::collections::HashSet;
use std::sync::Arc;

use data_encoding::HEXLOWER;
use futures::future::Either;
use futures::stream::BoxStream;
use futures::SinkExt;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use object_store::{path::Path, ObjectStore};
use prost::Message;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::LengthDelimitedCodec;
use tonic::async_trait;
use tracing::{instrument, trace, warn, Level};
use url::Url;

use super::{ClosureValidator, DirectoryPutter, DirectoryService};
use crate::{proto, B3Digest, Error};

/// Stores directory closures in an object store.
/// Notably, this makes use of the option to disallow accessing child directories except when
/// fetching them recursively via the top-level directory, since all batched writes
/// (using `put_multiple_start`) are stored in a single object.
/// Directories are stored in a length-delimited format with a 1MiB limit. The length field is a
/// u32 and the directories are stored in root-to-leaves topological order, the same way they will
/// be returned to the client in get_recursive.
#[derive(Clone)]
pub struct ObjectStoreDirectoryService {
    object_store: Arc<dyn ObjectStore>,
    base_path: Path,
}

#[instrument(level=Level::TRACE, skip_all,fields(base_path=%base_path,blob.digest=%digest),ret(Display))]
fn derive_dirs_path(base_path: &Path, digest: &B3Digest) -> Path {
    base_path
        .child("dirs")
        .child("b3")
        .child(HEXLOWER.encode(&digest.as_slice()[..2]))
        .child(HEXLOWER.encode(digest.as_slice()))
}

#[allow(clippy::identity_op)]
const MAX_FRAME_LENGTH: usize = 1 * 1024 * 1024 * 1000; // 1 MiB
                                                        //
impl ObjectStoreDirectoryService {
    /// Constructs a new [ObjectStoreBlobService] from a [Url] supported by
    /// [object_store].
    /// Any path suffix becomes the base path of the object store.
    /// additional options, the same as in [object_store::parse_url_opts] can
    /// be passed.
    pub fn parse_url_opts<I, K, V>(url: &Url, options: I) -> Result<Self, object_store::Error>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<String>,
    {
        let (object_store, path) = object_store::parse_url_opts(url, options)?;

        Ok(Self {
            object_store: Arc::new(object_store),
            base_path: path,
        })
    }

    /// Like [Self::parse_url_opts], except without the options.
    pub fn parse_url(url: &Url) -> Result<Self, object_store::Error> {
        Self::parse_url_opts(url, Vec::<(String, String)>::new())
    }
}

#[async_trait]
impl DirectoryService for ObjectStoreDirectoryService {
    /// This is the same steps as for get_recursive anyways, so we just call get_recursive and
    /// return the first element of the stream and drop the request.
    #[instrument(skip(self, digest), fields(directory.digest = %digest))]
    async fn get(&self, digest: &B3Digest) -> Result<Option<proto::Directory>, Error> {
        self.get_recursive(digest).take(1).next().await.transpose()
    }

    #[instrument(skip(self, directory), fields(directory.digest = %directory.digest()))]
    async fn put(&self, directory: proto::Directory) -> Result<B3Digest, Error> {
        if !directory.directories.is_empty() {
            return Err(Error::InvalidRequest(
                    "only put_multiple_start is supported by the ObjectStoreDirectoryService for directories with children".into(),
            ));
        }

        let mut handle = self.put_multiple_start();
        handle.put(directory).await?;
        handle.close().await
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> BoxStream<'static, Result<proto::Directory, Error>> {
        // The Directory digests we're expecting to receive.
        let mut expected_directory_digests: HashSet<B3Digest> =
            HashSet::from([root_directory_digest.clone()]);

        let dir_path = derive_dirs_path(&self.base_path, root_directory_digest);
        let object_store = self.object_store.clone();

        Box::pin(
            (async move {
                let stream = match object_store.get(&dir_path).await {
                    Ok(v) => v.into_stream(),
                    Err(object_store::Error::NotFound { .. }) => {
                        return Ok(Either::Left(futures::stream::empty()))
                    }
                    Err(e) => return Err(std::io::Error::from(e).into()),
                };

                // get a reader of the response body.
                let r = tokio_util::io::StreamReader::new(stream);
                let decompressed_stream = async_compression::tokio::bufread::ZstdDecoder::new(r);

                // the subdirectories are stored in a length delimited format
                let delimited_stream = LengthDelimitedCodec::builder()
                    .max_frame_length(MAX_FRAME_LENGTH)
                    .length_field_type::<u32>()
                    .new_read(decompressed_stream);

                let dirs_stream = delimited_stream.map_err(Error::from).and_then(move |buf| {
                    futures::future::ready((|| {
                        let mut hasher = blake3::Hasher::new();
                        let digest: B3Digest = hasher.update(&buf).finalize().as_bytes().into();

                        // Ensure to only decode the directory objects whose digests we trust
                        let was_expected = expected_directory_digests.remove(&digest);
                        if !was_expected {
                            return Err(crate::Error::StorageError(format!(
                                "received unexpected directory {}",
                                digest
                            )));
                        }

                        let directory = proto::Directory::decode(&*buf).map_err(|e| {
                            warn!("unable to parse directory {}: {}", digest, e);
                            Error::StorageError(e.to_string())
                        })?;

                        for directory in &directory.directories {
                            // Allow the children to appear next
                            expected_directory_digests.insert(
                                B3Digest::try_from(directory.digest.clone())
                                    .map_err(|e| Error::StorageError(e.to_string()))?,
                            );
                        }

                        Ok(directory)
                    })())
                });

                Ok(Either::Right(dirs_stream))
            })
            .try_flatten_stream(),
        )
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<(dyn DirectoryPutter + 'static)>
    where
        Self: Clone,
    {
        Box::new(ObjectStoreDirectoryPutter::new(
            self.object_store.clone(),
            self.base_path.clone(),
        ))
    }
}

struct ObjectStoreDirectoryPutter {
    object_store: Arc<dyn ObjectStore>,
    base_path: Path,

    directory_validator: Option<ClosureValidator>,
}

impl ObjectStoreDirectoryPutter {
    fn new(object_store: Arc<dyn ObjectStore>, base_path: Path) -> Self {
        Self {
            object_store,
            base_path,
            directory_validator: Some(Default::default()),
        }
    }
}

#[async_trait]
impl DirectoryPutter for ObjectStoreDirectoryPutter {
    #[instrument(level = "trace", skip_all, fields(directory.digest=%directory.digest()), err)]
    async fn put(&mut self, directory: proto::Directory) -> Result<(), Error> {
        match self.directory_validator {
            None => return Err(Error::StorageError("already closed".to_string())),
            Some(ref mut validator) => {
                validator.add(directory)?;
            }
        }

        Ok(())
    }

    #[instrument(level = "trace", skip_all, ret, err)]
    async fn close(&mut self) -> Result<B3Digest, Error> {
        let validator = match self.directory_validator.take() {
            None => return Err(Error::InvalidRequest("already closed".to_string())),
            Some(validator) => validator,
        };

        // retrieve the validated directories.
        // It is important that they are in topological order (root first),
        // since that's how we want to retrieve them from the object store in the end.
        let directories = validator.finalize_root_to_leaves()?;

        // Get the root digest
        let root_digest = directories
            .first()
            .ok_or_else(|| Error::InvalidRequest("got no directories".to_string()))?
            .digest();

        let dir_path = derive_dirs_path(&self.base_path, &root_digest);

        match self.object_store.head(&dir_path).await {
            // directory tree already exists, nothing to do
            Ok(_) => {
                trace!("directory tree already exists");
            }

            // directory tree does not yet exist, compress and upload.
            Err(object_store::Error::NotFound { .. }) => {
                trace!("uploading directory tree");

                let object_store_writer =
                    object_store::buffered::BufWriter::new(self.object_store.clone(), dir_path);
                let compressed_writer =
                    async_compression::tokio::write::ZstdEncoder::new(object_store_writer);
                let mut directories_sink = LengthDelimitedCodec::builder()
                    .max_frame_length(MAX_FRAME_LENGTH)
                    .length_field_type::<u32>()
                    .new_write(compressed_writer);

                for directory in directories {
                    directories_sink
                        .send(directory.encode_to_vec().into())
                        .await?;
                }

                let mut compressed_writer = directories_sink.into_inner();
                compressed_writer.shutdown().await?;
            }
            // other error
            Err(err) => Err(std::io::Error::from(err))?,
        }

        Ok(root_digest)
    }
}
