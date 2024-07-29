use crate::composition::{Registry, ServiceBuilder};
use crate::proto;
use crate::{B3Digest, Error};
use crate::{ValidateDirectoryError, ValidateNodeError};

use bytes::Bytes;
use futures::stream::BoxStream;
use tonic::async_trait;
mod combinators;
mod directory_graph;
mod from_addr;
mod grpc;
mod memory;
mod object_store;
mod order_validator;
mod redb;
mod simple_putter;
mod sled;
#[cfg(test)]
pub mod tests;
mod traverse;
mod utils;

pub use self::combinators::{Cache, CacheConfig};
pub use self::directory_graph::DirectoryGraph;
pub use self::from_addr::from_addr;
pub use self::grpc::{GRPCDirectoryService, GRPCDirectoryServiceConfig};
pub use self::memory::{MemoryDirectoryService, MemoryDirectoryServiceConfig};
pub use self::object_store::{ObjectStoreDirectoryService, ObjectStoreDirectoryServiceConfig};
pub use self::order_validator::{LeavesToRootValidator, OrderValidator, RootToLeavesValidator};
pub use self::redb::{RedbDirectoryService, RedbDirectoryServiceConfig};
pub use self::simple_putter::SimplePutter;
pub use self::sled::{SledDirectoryService, SledDirectoryServiceConfig};
pub use self::traverse::descend_to;
pub use self::utils::traverse_directory;

#[cfg(feature = "cloud")]
mod bigtable;

#[cfg(feature = "cloud")]
pub use self::bigtable::{BigtableDirectoryService, BigtableParameters};

/// The base trait all Directory services need to implement.
/// This is a simple get and put of [Directory], returning their
/// digest.
#[async_trait]
pub trait DirectoryService: Send + Sync {
    /// Looks up a single Directory message by its digest.
    /// The returned Directory message *must* be valid.
    /// In case the directory is not found, Ok(None) is returned.
    ///
    /// It is okay for certain implementations to only allow retrieval of
    /// Directory digests that are at the "root", aka the last element that's
    /// sent to a DirectoryPutter. This makes sense for implementations bundling
    /// closures of directories together in batches.
    async fn get(&self, digest: &B3Digest) -> Result<Option<Directory>, Error>;
    /// Uploads a single Directory message, and returns the calculated
    /// digest, or an error. An error *must* also be returned if the message is
    /// not valid.
    async fn put(&self, directory: Directory) -> Result<B3Digest, Error>;

    /// Looks up a closure of [Directory].
    /// Ideally this would be a `impl Stream<Item = Result<Directory, Error>>`,
    /// and we'd be able to add a default implementation for it here, but
    /// we can't have that yet.
    ///
    /// This returns a pinned, boxed stream. The pinning allows for it to be polled easily,
    /// and the box allows different underlying stream implementations to be returned since
    /// Rust doesn't support this as a generic in traits yet. This is the same thing that
    /// [async_trait] generates, but for streams instead of futures.
    ///
    /// The individually returned Directory messages *must* be valid.
    /// Directories are sent in an order from the root to the leaves, so that
    /// the receiving side can validate each message to be a connected to the root
    /// that has initially been requested.
    ///
    /// In case the directory can not be found, this should return an empty stream.
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> BoxStream<'static, Result<Directory, Error>>;

    /// Allows persisting a closure of [Directory], which is a graph of
    /// connected Directory messages.
    fn put_multiple_start(&self) -> Box<dyn DirectoryPutter>;
}

#[async_trait]
impl<A> DirectoryService for A
where
    A: AsRef<dyn DirectoryService> + Send + Sync,
{
    async fn get(&self, digest: &B3Digest) -> Result<Option<Directory>, Error> {
        self.as_ref().get(digest).await
    }

    async fn put(&self, directory: Directory) -> Result<B3Digest, Error> {
        self.as_ref().put(directory).await
    }

    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> BoxStream<'static, Result<Directory, Error>> {
        self.as_ref().get_recursive(root_directory_digest)
    }

    fn put_multiple_start(&self) -> Box<dyn DirectoryPutter> {
        self.as_ref().put_multiple_start()
    }
}

/// Provides a handle to put a closure of connected [Directory] elements.
///
/// The consumer can periodically call [DirectoryPutter::put], starting from the
/// leaves. Once the root is reached, [DirectoryPutter::close] can be called to
/// retrieve the root digest (or an error).
///
/// DirectoryPutters might be created without a single [DirectoryPutter::put],
/// and then dropped without calling [DirectoryPutter::close],
/// for example when ingesting a path that ends up not pointing to a directory,
/// but a single file or symlink.
#[async_trait]
pub trait DirectoryPutter: Send {
    /// Put a individual [Directory] into the store.
    /// Error semantics and behaviour is up to the specific implementation of
    /// this trait.
    /// Due to bursting, the returned error might refer to an object previously
    /// sent via `put`.
    async fn put(&mut self, directory: Directory) -> Result<(), Error>;

    /// Close the stream, and wait for any errors.
    /// If there's been any invalid Directory message uploaded, and error *must*
    /// be returned.
    async fn close(&mut self) -> Result<B3Digest, Error>;
}

/// Registers the builtin DirectoryService implementations with the registry
pub(crate) fn register_directory_services(reg: &mut Registry) {
    reg.register::<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>, super::directoryservice::ObjectStoreDirectoryServiceConfig>("objectstore");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>, super::directoryservice::MemoryDirectoryServiceConfig>("memory");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>, super::directoryservice::CacheConfig>("cache");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>, super::directoryservice::GRPCDirectoryServiceConfig>("grpc");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>, super::directoryservice::SledDirectoryServiceConfig>("sled");
    reg.register::<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>, super::directoryservice::RedbDirectoryServiceConfig>("redb");
    #[cfg(feature = "cloud")]
    {
        reg.register::<Box<dyn ServiceBuilder<Output = dyn DirectoryService>>, super::directoryservice::BigtableParameters>("bigtable");
    }
}

/// A Directory can contain Directory, File or Symlink nodes.
/// Each of these nodes have a name attribute, which is the basename in that
/// directory and node type specific attributes.
/// While a Node by itself may have any name, the names of Directory entries:
///  - MUST not contain slashes or null bytes
///  - MUST not be '.' or '..'
///  - MUST be unique across all three lists
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    nodes: Vec<Node>,
}

/// A DirectoryNode is a pointer to a [Directory], by its [Directory::digest].
/// It also gives it a `name` and `size`.
/// Such a node is either an element in the [Directory] it itself is contained in,
/// or a standalone root node./
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryNode {
    /// The (base)name of the directory
    name: Bytes,
    /// The blake3 hash of a Directory message, serialized in protobuf canonical form.
    digest: B3Digest,
    /// Number of child elements in the Directory referred to by `digest`.
    /// Calculated by summing up the numbers of nodes, and for each directory.
    /// its size field. Can be used for inode allocation.
    /// This field is precisely as verifiable as any other Merkle tree edge.
    /// Resolve `digest`, and you can compute it incrementally. Resolve the entire
    /// tree, and you can fully compute it from scratch.
    /// A credulous implementation won't reject an excessive size, but this is
    /// harmless: you'll have some ordinals without nodes. Undersizing is obvious
    /// and easy to reject: you won't have an ordinal for some nodes.
    size: u64,
}

impl DirectoryNode {
    pub fn new(name: Bytes, digest: B3Digest, size: u64) -> Result<Self, ValidateNodeError> {
        Ok(Self { name, digest, size })
    }

    pub fn digest(&self) -> &B3Digest {
        &self.digest
    }
    pub fn size(&self) -> u64 {
        self.size
    }
}

/// A FileNode represents a regular or executable file in a Directory or at the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    /// The (base)name of the file
    name: Bytes,

    /// The blake3 digest of the file contents
    digest: B3Digest,

    /// The file content size
    size: u64,

    /// Whether the file is executable
    executable: bool,
}

impl FileNode {
    pub fn new(
        name: Bytes,
        digest: B3Digest,
        size: u64,
        executable: bool,
    ) -> Result<Self, ValidateNodeError> {
        Ok(Self {
            name,
            digest,
            size,
            executable,
        })
    }

    pub fn digest(&self) -> &B3Digest {
        &self.digest
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn executable(&self) -> bool {
        self.executable
    }
}

/// A SymlinkNode represents a symbolic link in a Directory or at the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymlinkNode {
    /// The (base)name of the symlink
    name: Bytes,
    /// The target of the symlink.
    target: Bytes,
}

impl SymlinkNode {
    pub fn new(name: Bytes, target: Bytes) -> Result<Self, ValidateNodeError> {
        if target.is_empty() || target.contains(&b'\0') {
            return Err(ValidateNodeError::InvalidSymlinkTarget(target));
        }
        Ok(Self { name, target })
    }

    pub fn target(&self) -> &bytes::Bytes {
        &self.target
    }
}

/// A Node is either a [DirectoryNode], [FileNode] or [SymlinkNode].
/// While a Node by itself may have any name, only those matching specific requirements
/// can can be added as entries to a [Directory] (see the documentation on [Directory] for details).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Directory(DirectoryNode),
    File(FileNode),
    Symlink(SymlinkNode),
}

/// NamedNode is implemented for [FileNode], [DirectoryNode] and [SymlinkNode]
/// and [Node], so we can ask all of them for the name easily.
pub trait NamedNode {
    fn get_name(&self) -> &bytes::Bytes;
}

impl NamedNode for &FileNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
impl NamedNode for FileNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}

impl NamedNode for &DirectoryNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
impl NamedNode for DirectoryNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}

impl NamedNode for &SymlinkNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
impl NamedNode for SymlinkNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}

impl NamedNode for &Node {
    fn get_name(&self) -> &bytes::Bytes {
        match self {
            Node::File(node_file) => &node_file.name,
            Node::Directory(node_directory) => &node_directory.name,
            Node::Symlink(node_symlink) => &node_symlink.name,
        }
    }
}
impl NamedNode for Node {
    fn get_name(&self) -> &bytes::Bytes {
        match self {
            Node::File(node_file) => &node_file.name,
            Node::Directory(node_directory) => &node_directory.name,
            Node::Symlink(node_symlink) => &node_symlink.name,
        }
    }
}

impl Node {
    /// Returns the node with a new name.
    pub fn rename(self, name: bytes::Bytes) -> Self {
        match self {
            Node::Directory(n) => Node::Directory(DirectoryNode { name, ..n }),
            Node::File(n) => Node::File(FileNode { name, ..n }),
            Node::Symlink(n) => Node::Symlink(SymlinkNode { name, ..n }),
        }
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl PartialOrd for FileNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl PartialOrd for DirectoryNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DirectoryNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl PartialOrd for SymlinkNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SymlinkNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

fn checked_sum(iter: impl IntoIterator<Item = u64>) -> Option<u64> {
    iter.into_iter().try_fold(0u64, |acc, i| acc.checked_add(i))
}

impl Directory {
    pub fn new() -> Self {
        Directory { nodes: vec![] }
    }

    /// The size of a directory is the number of all regular and symlink elements,
    /// the number of directory elements, and their size fields.
    pub fn size(&self) -> u64 {
        // It's impossible to create a Directory where the size overflows, because we
        // check before every add() that the size won't overflow.
        (self.nodes.len() as u64) + self.directories().map(|e| e.size).sum::<u64>()
    }

    /// Calculates the digest of a Directory, which is the blake3 hash of a
    /// Directory protobuf message, serialized in protobuf canonical form.
    pub fn digest(&self) -> B3Digest {
        proto::Directory::from(self).digest()
    }

    /// Allows iterating over all nodes (directories, files and symlinks)
    /// ordered by their name.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> + Send + Sync + '_ {
        self.nodes.iter()
    }

    /// Allows iterating over the FileNode entries of this directory
    /// ordered by their name
    pub fn files(&self) -> impl Iterator<Item = &FileNode> + Send + Sync + '_ {
        self.nodes.iter().filter_map(|node| match node {
            Node::File(n) => Some(n),
            _ => None,
        })
    }

    /// Allows iterating over the subdirectories of this directory
    /// ordered by their name
    pub fn directories(&self) -> impl Iterator<Item = &DirectoryNode> + Send + Sync + '_ {
        self.nodes.iter().filter_map(|node| match node {
            Node::Directory(n) => Some(n),
            _ => None,
        })
    }

    /// Allows iterating over the SymlinkNode entries of this directory
    /// ordered by their name
    pub fn symlinks(&self) -> impl Iterator<Item = &SymlinkNode> + Send + Sync + '_ {
        self.nodes.iter().filter_map(|node| match node {
            Node::Symlink(n) => Some(n),
            _ => None,
        })
    }

    /// Checks a Node name for validity as a directory entry
    /// We disallow slashes, null bytes, '.', '..' and the empty string.
    pub(crate) fn validate_node_name(name: &[u8]) -> Result<(), ValidateNodeError> {
        if name.is_empty()
            || name == b".."
            || name == b"."
            || name.contains(&0x00)
            || name.contains(&b'/')
        {
            Err(ValidateNodeError::InvalidName(name.to_owned().into()))
        } else {
            Ok(())
        }
    }

    /// Adds the specified [Node] to the [Directory], preserving sorted entries.
    ///
    /// Inserting an element that already exists with the same name in the directory will yield an
    /// error.
    /// Inserting an element will validate that its name fulfills the stricter requirements for
    /// directory entries and yield an error if it is not.
    pub fn add(&mut self, node: Node) -> Result<(), ValidateDirectoryError> {
        Self::validate_node_name(node.get_name())
            .map_err(|e| ValidateDirectoryError::InvalidNode(node.get_name().clone().into(), e))?;

        // Check that the even after adding this new directory entry, the size calculation will not
        // overflow
        // FUTUREWORK: add some sort of batch add interface which only does this check once with
        // all the to-be-added entries
        checked_sum([
            self.size(),
            1,
            match node {
                Node::Directory(ref dir) => dir.size,
                _ => 0,
            },
        ])
        .ok_or(ValidateDirectoryError::SizeOverflow)?;

        // This assumes the [Directory] is sorted, since we don't allow accessing the nodes list
        // directly and all previous inserts should have been in-order
        let pos = match self
            .nodes
            .binary_search_by_key(&node.get_name(), |n| n.get_name())
        {
            Err(pos) => pos, // There is no node with this name; good!
            Ok(_) => {
                return Err(ValidateDirectoryError::DuplicateName(
                    node.get_name().to_vec(),
                ))
            }
        };

        self.nodes.insert(pos, node);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::{Directory, DirectoryNode, FileNode, Node, SymlinkNode};
    use crate::fixtures::DUMMY_DIGEST;
    use crate::ValidateDirectoryError;

    #[test]
    fn add_nodes_to_directory() {
        let mut d = Directory::new();

        d.add(Node::Directory(
            DirectoryNode::new("b".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();
        d.add(Node::Directory(
            DirectoryNode::new("a".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();
        d.add(Node::Directory(
            DirectoryNode::new("z".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();

        d.add(Node::File(
            FileNode::new("f".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
        ))
        .unwrap();
        d.add(Node::File(
            FileNode::new("c".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
        ))
        .unwrap();
        d.add(Node::File(
            FileNode::new("g".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
        ))
        .unwrap();

        d.add(Node::Symlink(
            SymlinkNode::new("t".into(), "a".into()).unwrap(),
        ))
        .unwrap();
        d.add(Node::Symlink(
            SymlinkNode::new("o".into(), "a".into()).unwrap(),
        ))
        .unwrap();
        d.add(Node::Symlink(
            SymlinkNode::new("e".into(), "a".into()).unwrap(),
        ))
        .unwrap();

        // Convert to proto struct and back to ensure we are not generating any invalid structures
        crate::directoryservice::Directory::try_from(crate::proto::Directory::from(d))
            .expect("directory should be valid");
    }

    #[test]
    fn validate_overflow() {
        let mut d = Directory::new();

        assert_eq!(
            d.add(Node::Directory(
                DirectoryNode::new("foo".into(), DUMMY_DIGEST.clone(), u64::MAX).unwrap(),
            )),
            Err(ValidateDirectoryError::SizeOverflow)
        );
    }

    #[test]
    fn add_duplicate_node_to_directory() {
        let mut d = Directory::new();

        d.add(Node::Directory(
            DirectoryNode::new("a".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();
        assert_eq!(
            format!(
                "{}",
                d.add(Node::File(
                    FileNode::new("a".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
                ))
                .expect_err("adding duplicate dir entry must fail")
            ),
            "\"a\" is a duplicate name"
        );
    }

    /// Attempt to add a directory entry with a name which should be rejected.
    #[tokio::test]
    async fn directory_reject_invalid_name() {
        let mut dir = Directory::new();
        assert!(
            dir.add(Node::Symlink(
                SymlinkNode::new(
                    "".into(), // wrong! can not be added to directory
                    "doesntmatter".into(),
                )
                .unwrap()
            ))
            .is_err(),
            "invalid symlink entry be rejected"
        );
    }
}
