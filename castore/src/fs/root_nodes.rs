use std::collections::BTreeMap;

use crate::{path::PathComponent, Error, Node};
use futures::stream::BoxStream;
use tonic::async_trait;

/// Provides an interface for looking up root nodes  in tvix-castore by given
/// a lookup key (usually the basename), and optionally allow a listing.
#[async_trait]
pub trait RootNodes: Send + Sync {
    /// Looks up a root CA node based on the basename of the node in the root
    /// directory of the filesystem.
    async fn get_by_basename(&self, name: &PathComponent) -> Result<Option<Node>, Error>;

    /// Lists all root CA nodes in the filesystem, as a tuple of (base)name
    /// and Node.
    /// An error can be returned in case listing is not allowed.
    fn list(&self) -> BoxStream<Result<(PathComponent, Node), Error>>;
}

#[async_trait]
/// Implements RootNodes for something deref'ing to a BTreeMap of Nodes, where
/// the key is the node name.
impl<T> RootNodes for T
where
    T: AsRef<BTreeMap<PathComponent, Node>> + Send + Sync,
{
    async fn get_by_basename(&self, name: &PathComponent) -> Result<Option<Node>, Error> {
        Ok(self.as_ref().get(name).cloned())
    }

    fn list(&self) -> BoxStream<Result<(PathComponent, Node), Error>> {
        Box::pin(tokio_stream::iter(
            self.as_ref()
                .iter()
                .map(|(name, node)| Ok((name.to_owned(), node.to_owned()))),
        ))
    }
}
