mod file_attr;
mod inode_tracker;
mod inodes;

#[cfg(feature = "fuse")]
pub mod fuse;

#[cfg(feature = "virtiofs")]
pub mod virtiofs;

#[cfg(test)]
mod tests;

use crate::pathinfoservice::PathInfoService;

use fuse_backend_rs::abi::fuse_abi::stat64;
use fuse_backend_rs::api::filesystem::{Context, FileSystem, FsOptions, ROOT_ID};
use futures::StreamExt;
use nix_compat::store_path::StorePath;
use parking_lot::RwLock;
use std::{
    collections::HashMap,
    io,
    str::FromStr,
    sync::atomic::AtomicU64,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncSeekExt},
    sync::mpsc,
};
use tracing::{debug, info_span, instrument, warn};
use tvix_castore::{
    blobservice::{BlobReader, BlobService},
    directoryservice::DirectoryService,
    proto::{node::Node, NamedNode},
    B3Digest, Error,
};

use self::{
    file_attr::{gen_file_attr, ROOT_FILE_ATTR},
    inode_tracker::InodeTracker,
    inodes::{DirectoryInodeData, InodeData},
};

/// This implements a read-only FUSE filesystem for a tvix-store
/// with the passed [BlobService], [DirectoryService] and [PathInfoService].
///
/// We don't allow listing on the root mountpoint (inode 0).
/// In the future, this might be made configurable once a listing method is
/// added to [self.path_info_service], and then show all store paths in that
/// store.
///
/// Linux uses inodes in filesystems. When implementing FUSE, most calls are
/// *for* a given inode.
///
/// This means, we need to have a stable mapping of inode numbers to the
/// corresponding store nodes.
///
/// We internally delegate all inode allocation and state keeping to the
/// inode tracker, and store the currently "explored" store paths together with
/// root inode of the root.
///
/// There's some places where inodes are allocated / data inserted into
/// the inode tracker, if not allocated before already:
///  - Processing a `lookup` request, either in the mount root, or somewhere
///    deeper
///  - Processing a `readdir` request
///
///  Things pointing to the same contents get the same inodes, irrespective of
///  their own location.
///  This means:
///  - Symlinks with the same target will get the same inode.
///  - Regular/executable files with the same contents will get the same inode
///  - Directories with the same contents will get the same inode.
///
/// Due to the above being valid across the whole store, and considering the
/// merkle structure is a DAG, not a tree, this also means we can't do "bucketed
/// allocation", aka reserve Directory.size inodes for each PathInfo.
pub struct TvixStoreFs {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,

    /// Whether to (try) listing elements in the root.
    list_root: bool,

    /// This maps a given StorePath to the inode we allocated for the root inode.
    store_paths: RwLock<HashMap<StorePath, u64>>,

    /// This keeps track of inodes and data alongside them.
    inode_tracker: RwLock<InodeTracker>,

    /// This holds all open file handles
    file_handles: RwLock<HashMap<u64, Arc<tokio::sync::Mutex<Box<dyn BlobReader>>>>>,

    next_file_handle: AtomicU64,

    tokio_handle: tokio::runtime::Handle,
}

impl TvixStoreFs {
    pub fn new(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
        path_info_service: Arc<dyn PathInfoService>,
        list_root: bool,
    ) -> Self {
        Self {
            blob_service,
            directory_service,
            path_info_service,

            list_root,

            store_paths: RwLock::new(HashMap::default()),
            inode_tracker: RwLock::new(Default::default()),

            file_handles: RwLock::new(Default::default()),
            next_file_handle: AtomicU64::new(1),
            tokio_handle: tokio::runtime::Handle::current(),
        }
    }

    /// This will turn a lookup request for [std::ffi::OsStr] in the root to
    /// a ino and [InodeData].
    /// It will peek in [self.store_paths], and then either look it up from
    /// [self.inode_tracker],
    /// or otherwise fetch from [self.path_info_service], and then insert into
    /// [self.inode_tracker].
    fn name_in_root_to_ino_and_data(
        &self,
        name: &std::ffi::CStr,
    ) -> Result<Option<(u64, Arc<InodeData>)>, Error> {
        // parse the name into a [StorePath].
        let store_path = if let Ok(name) = name.to_str() {
            match StorePath::from_str(name) {
                Ok(store_path) => store_path,
                Err(e) => {
                    debug!(e=?e, "unable to parse as store path");
                    // This is not an error, but a "ENOENT", as someone can stat
                    // a file inside the root that's no valid store path
                    return Ok(None);
                }
            }
        } else {
            debug!("{name:?} is not a valid utf-8 string");
            // same here.
            return Ok(None);
        };

        let ino = {
            // This extra scope makes sure we drop the read lock
            // immediately after reading, to prevent deadlocks.
            let store_paths = self.store_paths.read();
            store_paths.get(&store_path).cloned()
        };

        if let Some(ino) = ino {
            // If we already have that store path, lookup the inode from
            // self.store_paths and then get the data from [self.inode_tracker],
            // which in the case of a [InodeData::Directory] will be fully
            // populated.
            Ok(Some((
                ino,
                self.inode_tracker.read().get(ino).expect("must exist"),
            )))
        } else {
            // If we don't have it, look it up in PathInfoService.
            let path_info_service = self.path_info_service.clone();
            let task = self.tokio_handle.spawn({
                let digest = *store_path.digest();
                async move { path_info_service.get(digest).await }
            });
            match self.tokio_handle.block_on(task).unwrap()? {
                // the pathinfo doesn't exist, so the file doesn't exist.
                None => Ok(None),
                Some(path_info) => {
                    // The pathinfo does exist, so there must be a root node
                    let root_node = path_info.node.unwrap().node.unwrap();

                    // The name must match what's passed in the lookup, otherwise we return nothing.
                    if root_node.get_name() != store_path.to_string().as_bytes() {
                        return Ok(None);
                    }

                    // Let's check if someone else beat us to updating the inode tracker and
                    // store_paths map.
                    let mut store_paths = self.store_paths.write();
                    if let Some(ino) = store_paths.get(&store_path).cloned() {
                        return Ok(Some((
                            ino,
                            self.inode_tracker.read().get(ino).expect("must exist"),
                        )));
                    }

                    // insert the (sparse) inode data and register in
                    // self.store_paths.
                    // FUTUREWORK: change put to return the data after
                    // inserting, so we don't need to lookup a second
                    // time?
                    let (ino, inode) = {
                        let mut inode_tracker = self.inode_tracker.write();
                        let ino = inode_tracker.put((&root_node).into());
                        (ino, inode_tracker.get(ino).unwrap())
                    };
                    store_paths.insert(store_path, ino);

                    Ok(Some((ino, inode)))
                }
            }
        }
    }
}

impl FileSystem for TvixStoreFs {
    type Inode = u64;
    type Handle = u64;

    fn init(&self, _capable: FsOptions) -> io::Result<FsOptions> {
        Ok(FsOptions::empty())
    }

    #[tracing::instrument(skip_all, fields(rq.inode = inode))]
    fn getattr(
        &self,
        _ctx: &Context,
        inode: Self::Inode,
        _handle: Option<Self::Handle>,
    ) -> io::Result<(stat64, Duration)> {
        if inode == ROOT_ID {
            return Ok((ROOT_FILE_ATTR.into(), Duration::MAX));
        }

        match self.inode_tracker.read().get(inode) {
            None => Err(io::Error::from_raw_os_error(libc::ENOENT)),
            Some(node) => {
                debug!(node = ?node, "found node");
                Ok((gen_file_attr(&node, inode).into(), Duration::MAX))
            }
        }
    }

    #[tracing::instrument(skip_all, fields(rq.parent_inode = parent, rq.name = ?name))]
    fn lookup(
        &self,
        _ctx: &Context,
        parent: Self::Inode,
        name: &std::ffi::CStr,
    ) -> io::Result<fuse_backend_rs::api::filesystem::Entry> {
        debug!("lookup");

        // This goes from a parent inode to a node.
        // - If the parent is [ROOT_ID], we need to check
        //   [self.store_paths] (fetching from PathInfoService if needed)
        // - Otherwise, lookup the parent in [self.inode_tracker] (which must be
        //   a [InodeData::Directory]), and find the child with that name.
        if parent == ROOT_ID {
            return match self.name_in_root_to_ino_and_data(name) {
                Err(e) => {
                    warn!("{}", e);
                    Err(io::Error::from_raw_os_error(libc::ENOENT))
                }
                Ok(None) => Err(io::Error::from_raw_os_error(libc::ENOENT)),
                Ok(Some((ino, inode_data))) => {
                    debug!(inode_data=?&inode_data, ino=ino, "Some");
                    Ok(fuse_backend_rs::api::filesystem::Entry {
                        inode: ino,
                        attr: gen_file_attr(&inode_data, ino).into(),
                        attr_timeout: Duration::MAX,
                        entry_timeout: Duration::MAX,
                        ..Default::default()
                    })
                }
            };
        }

        // This is the "lookup for "a" inside inode 42.
        // We already know that inode 42 must be a directory.
        // It might not be populated yet, so if it isn't, we do (by
        // fetching from [self.directory_service]), and save the result in
        // [self.inode_tracker].
        // Now it for sure is populated, so we search for that name in the
        // list of children and return the FileAttrs.

        // TODO: Reduce the critical section of this write lock.
        let mut inode_tracker = self.inode_tracker.write();
        let parent_data = inode_tracker.get(parent).unwrap();
        let parent_data = match *parent_data {
            InodeData::Regular(..) | InodeData::Symlink(_) => {
                // if the parent inode was not a directory, this doesn't make sense
                return Err(io::Error::from_raw_os_error(libc::ENOTDIR));
            }
            InodeData::Directory(DirectoryInodeData::Sparse(ref parent_digest, _)) => {
                let directory_service = self.directory_service.clone();
                let parent_digest = parent_digest.to_owned();
                let task = self.tokio_handle.spawn(async move {
                    fetch_directory_inode_data(directory_service, &parent_digest).await
                });
                match self.tokio_handle.block_on(task).unwrap() {
                    Ok(new_data) => {
                        // update data in [self.inode_tracker] with populated variant.
                        // FUTUREWORK: change put to return the data after
                        // inserting, so we don't need to lookup a second
                        // time?
                        let ino = inode_tracker.put(new_data);
                        inode_tracker.get(ino).unwrap()
                    }
                    Err(_e) => {
                        return Err(io::Error::from_raw_os_error(libc::EIO));
                    }
                }
            }
            InodeData::Directory(DirectoryInodeData::Populated(..)) => parent_data,
        };

        // now parent_data can only be a [InodeData::Directory(DirectoryInodeData::Populated(..))].
        let (parent_digest, children) = if let InodeData::Directory(
            DirectoryInodeData::Populated(ref parent_digest, ref children),
        ) = *parent_data
        {
            (parent_digest, children)
        } else {
            panic!("unexpected type")
        };
        let span = info_span!("lookup", directory.digest = %parent_digest);
        let _enter = span.enter();

        // in the children, find the one with the desired name.
        if let Some((child_ino, _)) = children.iter().find(|e| e.1.get_name() == name.to_bytes()) {
            // lookup the child [InodeData] in [self.inode_tracker].
            // We know the inodes for children have already been allocated.
            let child_inode_data = inode_tracker.get(*child_ino).unwrap();

            // Reply with the file attributes for the child.
            // For child directories, we still have all data we need to reply.
            Ok(fuse_backend_rs::api::filesystem::Entry {
                inode: *child_ino,
                attr: gen_file_attr(&child_inode_data, *child_ino).into(),
                attr_timeout: Duration::MAX,
                entry_timeout: Duration::MAX,
                ..Default::default()
            })
        } else {
            // Child not found, return ENOENT.
            Err(io::Error::from_raw_os_error(libc::ENOENT))
        }
    }

    // TODO: readdirplus?

    #[tracing::instrument(skip_all, fields(rq.inode = inode, rq.offset = offset))]
    fn readdir(
        &self,
        _ctx: &Context,
        inode: Self::Inode,
        _handle: Self::Handle,
        _size: u32,
        offset: u64,
        add_entry: &mut dyn FnMut(fuse_backend_rs::api::filesystem::DirEntry) -> io::Result<usize>,
    ) -> io::Result<()> {
        debug!("readdir");

        if inode == ROOT_ID {
            if !self.list_root {
                return Err(io::Error::from_raw_os_error(libc::EPERM)); // same error code as ipfs/kubo
            } else {
                let path_info_service = self.path_info_service.clone();
                let (tx, mut rx) = mpsc::channel(16);

                // This task will run in the background immediately and will exit
                // after the stream ends or if we no longer want any more entries.
                self.tokio_handle.spawn(async move {
                    let mut stream = path_info_service.list().skip(offset as usize).enumerate();
                    while let Some(path_info) = stream.next().await {
                        if tx.send(path_info).await.is_err() {
                            // If we get a send error, it means the sync code
                            // doesn't want any more entries.
                            break;
                        }
                    }
                });

                while let Some((i, path_info)) = rx.blocking_recv() {
                    let path_info = match path_info {
                        Err(e) => {
                            warn!("failed to retrieve pathinfo: {}", e);
                            return Err(io::Error::from_raw_os_error(libc::EPERM));
                        }
                        Ok(path_info) => path_info,
                    };

                    // We know the root node exists and the store_path can be parsed because clients MUST validate.
                    let root_node = path_info.node.unwrap().node.unwrap();
                    let store_path = StorePath::from_bytes(root_node.get_name()).unwrap();

                    let ino = {
                        // This extra scope makes sure we drop the read lock
                        // immediately after reading, to prevent deadlocks.
                        let store_paths = self.store_paths.read();
                        store_paths.get(&store_path).cloned()
                    };
                    let ino = match ino {
                        Some(ino) => ino,
                        None => {
                            // insert the (sparse) inode data and register in
                            // self.store_paths.
                            let ino = self.inode_tracker.write().put((&root_node).into());
                            self.store_paths.write().insert(store_path.clone(), ino);
                            ino
                        }
                    };

                    let ty = match root_node {
                        Node::Directory(_) => libc::S_IFDIR,
                        Node::File(_) => libc::S_IFREG,
                        Node::Symlink(_) => libc::S_IFLNK,
                    };

                    let written = add_entry(fuse_backend_rs::api::filesystem::DirEntry {
                        ino,
                        offset: offset + i as u64 + 1,
                        type_: ty,
                        name: store_path.to_string().as_bytes(),
                    })?;
                    // If the buffer is full, add_entry will return `Ok(0)`.
                    if written == 0 {
                        break;
                    }
                }

                return Ok(());
            }
        }

        // lookup the inode data.
        let mut inode_tracker = self.inode_tracker.write();
        let dir_inode_data = inode_tracker.get(inode).unwrap();
        let dir_inode_data = match *dir_inode_data {
            InodeData::Regular(..) | InodeData::Symlink(..) => {
                warn!("Not a directory");
                return Err(io::Error::from_raw_os_error(libc::ENOTDIR));
            }
            InodeData::Directory(DirectoryInodeData::Sparse(ref directory_digest, _)) => {
                let directory_digest = directory_digest.to_owned();
                let directory_service = self.directory_service.clone();
                let task = self.tokio_handle.spawn(async move {
                    fetch_directory_inode_data(directory_service, &directory_digest).await
                });
                match self.tokio_handle.block_on(task).unwrap() {
                    Ok(new_data) => {
                        // update data in [self.inode_tracker] with populated variant.
                        // FUTUREWORK: change put to return the data after
                        // inserting, so we don't need to lookup a second
                        // time?
                        let ino = inode_tracker.put(new_data.clone());
                        inode_tracker.get(ino).unwrap()
                    }
                    Err(_e) => {
                        return Err(io::Error::from_raw_os_error(libc::EIO));
                    }
                }
            }
            InodeData::Directory(DirectoryInodeData::Populated(..)) => dir_inode_data,
        };

        // now parent_data can only be InodeData::Directory(DirectoryInodeData::Populated(..))
        if let InodeData::Directory(DirectoryInodeData::Populated(ref _digest, ref children)) =
            *dir_inode_data
        {
            for (i, (ino, child_node)) in children.iter().skip(offset as usize).enumerate() {
                // the second parameter will become the "offset" parameter on the next call.
                let written = add_entry(fuse_backend_rs::api::filesystem::DirEntry {
                    ino: *ino,
                    offset: offset + i as u64 + 1,
                    type_: match child_node {
                        #[allow(clippy::unnecessary_cast)]
                        // libc::S_IFDIR is u32 on Linux and u16 on MacOS
                        Node::Directory(_) => libc::S_IFDIR as u32,
                        #[allow(clippy::unnecessary_cast)]
                        // libc::S_IFDIR is u32 on Linux and u16 on MacOS
                        Node::File(_) => libc::S_IFREG as u32,
                        #[allow(clippy::unnecessary_cast)]
                        // libc::S_IFDIR is u32 on Linux and u16 on MacOS
                        Node::Symlink(_) => libc::S_IFLNK as u32,
                    },
                    name: child_node.get_name(),
                })?;
                // If the buffer is full, add_entry will return `Ok(0)`.
                if written == 0 {
                    break;
                }
            }
        } else {
            panic!("unexpected type")
        }

        Ok(())
    }

    #[tracing::instrument(skip_all, fields(rq.inode = inode))]
    fn open(
        &self,
        _ctx: &Context,
        inode: Self::Inode,
        _flags: u32,
        _fuse_flags: u32,
    ) -> io::Result<(
        Option<Self::Handle>,
        fuse_backend_rs::api::filesystem::OpenOptions,
    )> {
        if inode == ROOT_ID {
            return Err(io::Error::from_raw_os_error(libc::ENOSYS));
        }

        // lookup the inode
        match *self.inode_tracker.read().get(inode).unwrap() {
            // read is invalid on non-files.
            InodeData::Directory(..) | InodeData::Symlink(_) => {
                warn!("is directory");
                Err(io::Error::from_raw_os_error(libc::EISDIR))
            }
            InodeData::Regular(ref blob_digest, _blob_size, _) => {
                let span = info_span!("read", blob.digest = %blob_digest);
                let _enter = span.enter();

                let blob_service = self.blob_service.clone();
                let blob_digest = blob_digest.clone();

                let task = self
                    .tokio_handle
                    .spawn(async move { blob_service.open_read(&blob_digest).await });

                let blob_reader = self.tokio_handle.block_on(task).unwrap();

                match blob_reader {
                    Ok(None) => {
                        warn!("blob not found");
                        Err(io::Error::from_raw_os_error(libc::EIO))
                    }
                    Err(e) => {
                        warn!(e=?e, "error opening blob");
                        Err(io::Error::from_raw_os_error(libc::EIO))
                    }
                    Ok(Some(blob_reader)) => {
                        // get a new file handle
                        // TODO: this will overflow after 2**64 operations,
                        // which is fine for now.
                        // See https://cl.tvl.fyi/c/depot/+/8834/comment/a6684ce0_d72469d1
                        // for the discussion on alternatives.
                        let fh = self.next_file_handle.fetch_add(1, Ordering::SeqCst);

                        debug!("add file handle {}", fh);
                        self.file_handles
                            .write()
                            .insert(fh, Arc::new(tokio::sync::Mutex::new(blob_reader)));

                        Ok((
                            Some(fh),
                            fuse_backend_rs::api::filesystem::OpenOptions::empty(),
                        ))
                    }
                }
            }
        }
    }

    #[tracing::instrument(skip_all, fields(rq.inode = inode, fh = handle))]
    fn release(
        &self,
        _ctx: &Context,
        inode: Self::Inode,
        _flags: u32,
        handle: Self::Handle,
        _flush: bool,
        _flock_release: bool,
        _lock_owner: Option<u64>,
    ) -> io::Result<()> {
        // remove and get ownership on the blob reader
        match self.file_handles.write().remove(&handle) {
            // drop it, which will close it.
            Some(blob_reader) => drop(blob_reader),
            None => {
                // These might already be dropped if a read error occured.
                debug!("file_handle {} not found", handle);
            }
        }

        Ok(())
    }

    #[tracing::instrument(skip_all, fields(rq.inode = inode, rq.offset = offset, rq.size = size))]
    fn read(
        &self,
        _ctx: &Context,
        inode: Self::Inode,
        handle: Self::Handle,
        w: &mut dyn fuse_backend_rs::api::filesystem::ZeroCopyWriter,
        size: u32,
        offset: u64,
        _lock_owner: Option<u64>,
        _flags: u32,
    ) -> io::Result<usize> {
        debug!("read");

        // We need to take out the blob reader from self.file_handles, so we can
        // interact with it in the separate task.
        // On success, we pass it back out of the task, so we can put it back in self.file_handles.
        let blob_reader = match self.file_handles.read().get(&handle) {
            Some(blob_reader) => blob_reader.clone(),
            None => {
                warn!("file handle {} unknown", handle);
                return Err(io::Error::from_raw_os_error(libc::EIO));
            }
        };

        let task = self.tokio_handle.spawn(async move {
            let mut blob_reader = blob_reader.lock().await;

            // seek to the offset specified, which is relative to the start of the file.
            let resp = blob_reader.seek(io::SeekFrom::Start(offset)).await;

            match resp {
                Ok(pos) => {
                    debug_assert_eq!(offset, pos);
                }
                Err(e) => {
                    warn!("failed to seek to offset {}: {}", offset, e);
                    return Err(io::Error::from_raw_os_error(libc::EIO));
                }
            }

            // As written in the fuse docs, read should send exactly the number
            // of bytes requested except on EOF or error.

            let mut buf: Vec<u8> = Vec::with_capacity(size as usize);

            while (buf.len() as u64) < size as u64 {
                let int_buf = blob_reader.fill_buf().await?;
                // copy things from the internal buffer into buf to fill it till up until size

                // an empty buffer signals we reached EOF.
                if int_buf.is_empty() {
                    break;
                }

                // calculate how many bytes we can read from int_buf.
                // It's either all of int_buf, or the number of bytes missing in buf to reach size.
                let len_to_copy = std::cmp::min(int_buf.len(), size as usize - buf.len());

                // copy these bytes into our buffer
                buf.extend_from_slice(&int_buf[..len_to_copy]);
                // and consume them in the buffered reader.
                blob_reader.consume(len_to_copy);
            }

            Ok(buf)
        });

        let buf = self.tokio_handle.block_on(task).unwrap()?;

        w.write(&buf)
    }

    #[tracing::instrument(skip_all, fields(rq.inode = inode))]
    fn readlink(&self, _ctx: &Context, inode: Self::Inode) -> io::Result<Vec<u8>> {
        if inode == ROOT_ID {
            return Err(io::Error::from_raw_os_error(libc::ENOSYS));
        }

        // lookup the inode
        match *self.inode_tracker.read().get(inode).unwrap() {
            InodeData::Directory(..) | InodeData::Regular(..) => {
                Err(io::Error::from_raw_os_error(libc::EINVAL))
            }
            InodeData::Symlink(ref target) => Ok(target.to_vec()),
        }
    }
}

/// This will lookup a directory by digest, and will turn it into a
/// [InodeData::Directory(DirectoryInodeData::Populated(..))].
/// This is both used to initially insert the root node of a store path,
/// as well as when looking up an intermediate DirectoryNode.
#[instrument(skip_all, fields(directory.digest = %directory_digest), err)]
async fn fetch_directory_inode_data<DS: DirectoryService + ?Sized>(
    directory_service: Arc<DS>,
    directory_digest: &B3Digest,
) -> Result<InodeData, Error> {
    match directory_service.get(directory_digest).await? {
        // If the Directory can't be found, this is a hole, bail out.
        None => Err(Error::StorageError(format!(
            "directory {} not found",
            directory_digest
        ))),
        Some(directory) => Ok(directory.into()),
    }
}
