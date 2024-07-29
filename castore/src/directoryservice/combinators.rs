use std::sync::Arc;

use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use tonic::async_trait;
use tracing::{instrument, trace};

use super::{Directory, DirectoryGraph, DirectoryService, RootToLeavesValidator, SimplePutter};
use crate::composition::{CompositionContext, ServiceBuilder};
use crate::directoryservice::DirectoryPutter;
use crate::B3Digest;
use crate::Error;

/// Asks near first, if not found, asks far.
/// If found in there, returns it, and *inserts* it into
/// near.
/// Specifically, it always obtains the entire directory closure from far and inserts it into near,
/// which is useful when far does not support accessing intermediate directories (but near does).
/// There is no negative cache.
/// Inserts and listings are not implemented for now.
#[derive(Clone)]
pub struct Cache<DS1, DS2> {
    near: DS1,
    far: DS2,
}

impl<DS1, DS2> Cache<DS1, DS2> {
    pub fn new(near: DS1, far: DS2) -> Self {
        Self { near, far }
    }
}

#[async_trait]
impl<DS1, DS2> DirectoryService for Cache<DS1, DS2>
where
    DS1: DirectoryService + Clone + 'static,
    DS2: DirectoryService + Clone + 'static,
{
    #[instrument(skip(self, digest), fields(directory.digest = %digest))]
    async fn get(&self, digest: &B3Digest) -> Result<Option<Directory>, Error> {
        match self.near.get(digest).await? {
            Some(directory) => {
                trace!("serving from cache");
                Ok(Some(directory))
            }
            None => {
                trace!("not found in near, asking remote…");

                let mut copy = DirectoryGraph::with_order(
                    RootToLeavesValidator::new_with_root_digest(digest.clone()),
                );

                let mut stream = self.far.get_recursive(digest);
                let root = stream.try_next().await?;

                if let Some(root) = root.clone() {
                    copy.add(root)
                        .map_err(|e| Error::StorageError(e.to_string()))?;
                }

                while let Some(dir) = stream.try_next().await? {
                    copy.add(dir)
                        .map_err(|e| Error::StorageError(e.to_string()))?;
                }

                let copy = copy
                    .validate()
                    .map_err(|e| Error::StorageError(e.to_string()))?;

                let mut put = self.near.put_multiple_start();
                for dir in copy.drain_leaves_to_root() {
                    put.put(dir).await?;
                }
                put.close().await?;

                Ok(root)
            }
        }
    }

    #[instrument(skip_all)]
    async fn put(&self, _directory: Directory) -> Result<B3Digest, Error> {
        Err(Error::StorageError("unimplemented".to_string()))
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> BoxStream<'static, Result<Directory, Error>> {
        let near = self.near.clone();
        let far = self.far.clone();
        let digest = root_directory_digest.clone();
        Box::pin(
            (async move {
                let mut stream = near.get_recursive(&digest);
                match stream.try_next().await? {
                    Some(first) => {
                        trace!("serving from cache");
                        Ok(futures::stream::once(async { Ok(first) })
                            .chain(stream)
                            .left_stream())
                    }
                    None => {
                        trace!("not found in near, asking remote…");

                        let mut copy_for_near = DirectoryGraph::with_order(
                            RootToLeavesValidator::new_with_root_digest(digest.clone()),
                        );
                        let mut copy_for_client = vec![];

                        let mut stream = far.get_recursive(&digest);
                        while let Some(dir) = stream.try_next().await? {
                            copy_for_near
                                .add(dir.clone())
                                .map_err(|e| Error::StorageError(e.to_string()))?;
                            copy_for_client.push(dir);
                        }

                        let copy_for_near = copy_for_near
                            .validate()
                            .map_err(|e| Error::StorageError(e.to_string()))?;
                        let mut put = near.put_multiple_start();
                        for dir in copy_for_near.drain_leaves_to_root() {
                            put.put(dir).await?;
                        }
                        put.close().await?;

                        Ok(futures::stream::iter(copy_for_client.into_iter().map(Ok))
                            .right_stream())
                    }
                }
            })
            .try_flatten_stream(),
        )
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<(dyn DirectoryPutter + 'static)> {
        Box::new(SimplePutter::new((*self).clone()))
    }
}

#[derive(serde::Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    near: String,
    far: String,
}

impl TryFrom<url::Url> for CacheConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(_url: url::Url) -> Result<Self, Self::Error> {
        Err(Error::StorageError(
            "Instantiating a CombinedDirectoryService from a url is not supported".into(),
        )
        .into())
    }
}

#[async_trait]
impl ServiceBuilder for CacheConfig {
    type Output = dyn DirectoryService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        context: &CompositionContext,
    ) -> Result<Arc<dyn DirectoryService>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let (near, far) = futures::join!(
            context.resolve(self.near.clone()),
            context.resolve(self.far.clone())
        );
        Ok(Arc::new(Cache {
            near: near?,
            far: far?,
        }))
    }
}
