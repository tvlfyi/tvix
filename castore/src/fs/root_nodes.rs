use std::{collections::BTreeMap, ops::Deref, pin::Pin};

use crate::{proto::node::Node, Error};
use bytes::Bytes;
use futures::Stream;
use tonic::async_trait;

/// Provides an interface for looking up root nodes  in tvix-castore by given
/// a lookup key (usually the basename), and optionally allow a listing.
#[async_trait]
pub trait RootNodes: Send + Sync {
    /// Looks up a root CA node based on the basename of the node in the root
    /// directory of the filesystem.
    async fn get_by_basename(&self, name: &[u8]) -> Result<Option<Node>, Error>;

    /// Lists all root CA nodes in the filesystem. An error can be returned
    /// in case listing is not allowed
    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<Node, Error>> + Send + '_>>;
}

#[async_trait]
/// Implements RootNodes for something deref'ing to a BTreeMap of Nodes, where
/// the key is the node name.
impl<T> RootNodes for T
where
    T: Deref<Target = BTreeMap<Bytes, Node>> + Send + Sync,
{
    async fn get_by_basename(&self, name: &[u8]) -> Result<Option<Node>, Error> {
        Ok(self.get(name).cloned())
    }

    fn list(&self) -> Pin<Box<dyn Stream<Item = Result<Node, Error>> + Send + '_>> {
        Box::pin(tokio_stream::iter(self.iter().map(|(_, v)| Ok(v.clone()))))
    }
}
