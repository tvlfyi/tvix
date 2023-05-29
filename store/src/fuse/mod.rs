mod file_attr;
mod inode_tracker;
mod inodes;

#[cfg(test)]
mod tests;

use crate::{
    blobservice::BlobService,
    directoryservice::DirectoryService,
    fuse::{
        file_attr::gen_file_attr,
        inodes::{DirectoryInodeData, InodeData},
    },
    pathinfoservice::PathInfoService,
    proto::{node::Node, NamedNode},
    B3Digest, Error,
};
use fuser::{FileAttr, ReplyAttr, Request};
use nix_compat::store_path::StorePath;
use std::io::Read;
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};
use tracing::{debug, info_span, warn};

use self::inode_tracker::InodeTracker;

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
/// We internally delegate all inode allocation and state keeping to a
/// [InodeTracker], and store the currently "explored" store paths together with
/// root inode of the root.
///
/// There's some places where inodes are allocated / data inserted into
/// [self.inode_tracker], if not allocated before already:
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
pub struct FUSE {
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    path_info_service: Arc<dyn PathInfoService>,

    /// This maps a given StorePath to the inode we allocated for the root inode.
    store_paths: HashMap<StorePath, u64>,

    /// This keeps track of inodes and data alongside them.
    inode_tracker: InodeTracker,
}

impl FUSE {
    pub fn new(
        blob_service: Arc<dyn BlobService>,
        directory_service: Arc<dyn DirectoryService>,
        path_info_service: Arc<dyn PathInfoService>,
    ) -> Self {
        Self {
            blob_service,
            directory_service,
            path_info_service,

            store_paths: HashMap::default(),
            inode_tracker: Default::default(),
        }
    }

    /// This will turn a lookup request for [std::ffi::OsStr] in the root to
    /// a ino and [InodeData].
    /// It will peek in [self.store_paths], and then either look it up from
    /// [self.inode_tracker],
    /// or otherwise fetch from [self.path_info_service], and then insert into
    /// [self.inode_tracker].
    fn name_in_root_to_ino_and_data(
        &mut self,
        name: &std::ffi::OsStr,
    ) -> Result<Option<(u64, Arc<InodeData>)>, Error> {
        // parse the name into a [StorePath].
        let store_path = if let Some(name) = name.to_str() {
            match StorePath::from_string(name) {
                Ok(store_path) => store_path,
                Err(e) => {
                    debug!(e=?e, "unable to parse as store path");
                    // This is not an error, but a "ENOENT", as someone can stat
                    // a file inside the root that's no valid store path
                    return Ok(None);
                }
            }
        } else {
            debug!("{name:?} is no string");
            // same here.
            return Ok(None);
        };

        if let Some(ino) = self.store_paths.get(&store_path) {
            // If we already have that store path, lookup the inode from
            // self.store_paths and then get the data from [self.inode_tracker],
            // which in the case of a [InodeData::Directory] will be fully
            // populated.
            Ok(Some((
                *ino,
                self.inode_tracker.get(*ino).expect("must exist"),
            )))
        } else {
            // If we don't have it, look it up in PathInfoService.
            match self.path_info_service.get(store_path.digest)? {
                // the pathinfo doesn't exist, so the file doesn't exist.
                None => Ok(None),
                Some(path_info) => {
                    // The pathinfo does exist, so there must be a root node
                    let root_node = path_info.node.unwrap().node.unwrap();

                    // The name must match what's passed in the lookup, otherwise we return nothing.
                    if root_node.get_name() != store_path.to_string() {
                        return Ok(None);
                    }

                    // insert the (sparse) inode data and register in
                    // self.store_paths.
                    // FUTUREWORK: change put to return the data after
                    // inserting, so we don't need to lookup a second
                    // time?
                    let ino = self.inode_tracker.put((&root_node).into());
                    self.store_paths.insert(store_path, ino);

                    Ok(Some((ino, self.inode_tracker.get(ino).unwrap())))
                }
            }
        }
    }

    /// This will lookup a directory by digest, and will turn it into a
    /// [InodeData::Directory(DirectoryInodeData::Populated(..))].
    /// This is both used to initially insert the root node of a store path,
    /// as well as when looking up an intermediate DirectoryNode.
    fn fetch_directory_inode_data(&self, directory_digest: &B3Digest) -> Result<InodeData, Error> {
        match self.directory_service.get(directory_digest) {
            Err(e) => {
                warn!(e = e.to_string(), directory.digest=%directory_digest, "failed to get directory");
                Err(e)
            }
            // If the Directory can't be found, this is a hole, bail out.
            Ok(None) => {
                tracing::error!(directory.digest=%directory_digest, "directory not found in directory service");
                Err(Error::StorageError(format!(
                    "directory {} not found",
                    directory_digest
                )))
            }
            Ok(Some(directory)) => Ok(directory.into()),
        }
    }
}

impl fuser::Filesystem for FUSE {
    #[tracing::instrument(skip_all, fields(rq.inode = ino))]
    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        debug!("getattr");

        if ino == fuser::FUSE_ROOT_ID {
            reply.attr(&Duration::MAX, &file_attr::ROOT_FILE_ATTR);
            return;
        }

        match self.inode_tracker.get(ino) {
            None => reply.error(libc::ENOENT),
            Some(node) => {
                debug!(node = ?node, "found node");
                reply.attr(&Duration::MAX, &file_attr::gen_file_attr(&node, ino));
            }
        }
    }

    #[tracing::instrument(skip_all, fields(rq.parent_inode = parent_ino, rq.name = ?name))]
    fn lookup(
        &mut self,
        _req: &Request,
        parent_ino: u64,
        name: &std::ffi::OsStr,
        reply: fuser::ReplyEntry,
    ) {
        debug!("lookup");

        // This goes from a parent inode to a node.
        // - If the parent is [fuser::FUSE_ROOT_ID], we need to check
        //   [self.store_paths] (fetching from PathInfoService if needed)
        // - Otherwise, lookup the parent in [self.inode_tracker] (which must be
        //   a [InodeData::Directory]), and find the child with that name.
        if parent_ino == fuser::FUSE_ROOT_ID {
            match self.name_in_root_to_ino_and_data(name) {
                Err(e) => {
                    warn!("{}", e);
                    reply.error(libc::EIO);
                }
                Ok(None) => {
                    reply.error(libc::ENOENT);
                }
                Ok(Some((ino, inode_data))) => {
                    warn!(inode_data=?&inode_data, ino=ino, "Some");
                    reply_with_entry(reply, &gen_file_attr(&inode_data, ino));
                }
            }
        } else {
            // This is the "lookup for "a" inside inode 42.
            // We already know that inode 42 must be a directory.
            // It might not be populated yet, so if it isn't, we do (by
            // fetching from [self.directory_service]), and save the result in
            // [self.inode_tracker].
            // Now it for sure is populated, so we search for that name in the
            // list of children and return the FileAttrs.

            let parent_data = self.inode_tracker.get(parent_ino).unwrap();
            let parent_data = match *parent_data {
                InodeData::Regular(..) | InodeData::Symlink(_) => {
                    // if the parent inode was not a directory, this doesn't make sense
                    reply.error(libc::ENOTDIR);
                    return;
                }
                InodeData::Directory(DirectoryInodeData::Sparse(ref parent_digest, _)) => {
                    match self.fetch_directory_inode_data(parent_digest) {
                        Ok(new_data) => {
                            // update data in [self.inode_tracker] with populated variant.
                            // FUTUREWORK: change put to return the data after
                            // inserting, so we don't need to lookup a second
                            // time?
                            let ino = self.inode_tracker.put(new_data);
                            self.inode_tracker.get(ino).unwrap()
                        }
                        Err(_e) => {
                            reply.error(libc::EIO);
                            return;
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
            if let Some((child_ino, _)) = children.iter().find(|e| e.1.get_name() == name) {
                // lookup the child [InodeData] in [self.inode_tracker].
                // We know the inodes for children have already been allocated.
                let child_inode_data = self.inode_tracker.get(*child_ino).unwrap();

                // Reply with the file attributes for the child.
                // For child directories, we still have all data we need to reply.
                reply_with_entry(reply, &gen_file_attr(&child_inode_data, *child_ino));
            } else {
                // Child not found, return ENOENT.
                reply.error(libc::ENOENT);
            }
        }
    }

    // TODO: readdirplus?

    #[tracing::instrument(skip_all, fields(rq.inode = ino, rq.offset = offset))]
    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        debug!("readdir");

        if ino == fuser::FUSE_ROOT_ID {
            reply.error(libc::EPERM); // same error code as ipfs/kubo
            return;
        }

        // lookup the inode data.
        let dir_inode_data = self.inode_tracker.get(ino).unwrap();
        let dir_inode_data = match *dir_inode_data {
            InodeData::Regular(..) | InodeData::Symlink(..) => {
                warn!("Not a directory");
                reply.error(libc::ENOTDIR);
                return;
            }
            InodeData::Directory(DirectoryInodeData::Sparse(ref directory_digest, _)) => {
                match self.fetch_directory_inode_data(directory_digest) {
                    Ok(new_data) => {
                        // update data in [self.inode_tracker] with populated variant.
                        // FUTUREWORK: change put to return the data after
                        // inserting, so we don't need to lookup a second
                        // time?
                        let ino = self.inode_tracker.put(new_data);
                        self.inode_tracker.get(ino).unwrap()
                    }
                    Err(_e) => {
                        reply.error(libc::EIO);
                        return;
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
                let full = reply.add(
                    *ino,
                    offset + i as i64 + 1_i64,
                    match child_node {
                        Node::Directory(_) => fuser::FileType::Directory,
                        Node::File(_) => fuser::FileType::RegularFile,
                        Node::Symlink(_) => fuser::FileType::Symlink,
                    },
                    child_node.get_name(),
                );
                if full {
                    break;
                }
            }
            reply.ok();
        } else {
            panic!("unexpected type")
        }
    }

    /// TODO: implement open + close?

    #[tracing::instrument(skip_all, fields(rq.inode = ino, rq.offset = offset, rq.size = size))]
    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        debug!("read");

        if ino == fuser::FUSE_ROOT_ID {
            reply.error(libc::ENOSYS);
            return;
        }
        // lookup the inode
        match *self.inode_tracker.get(ino).unwrap() {
            // read is invalid on non-files.
            InodeData::Directory(..) | InodeData::Symlink(_) => {
                warn!("is directory");
                reply.error(libc::EISDIR);
            }
            InodeData::Regular(ref blob_digest, _blob_size, _) => {
                let span = info_span!("read", blob.digest = %blob_digest);
                let _enter = span.enter();

                match self.blob_service.open_read(blob_digest) {
                    Ok(None) => {
                        warn!("blob not found");
                        reply.error(libc::EIO);
                    }
                    Err(e) => {
                        warn!(e=?e, "error opening blob");
                        reply.error(libc::EIO);
                    }
                    Ok(Some(blob_reader)) => {
                        let data: std::io::Result<Vec<u8>> = blob_reader
                            .bytes()
                            // TODO: this is obviously terrible. blobreader should implement seek.
                            .skip(offset.try_into().unwrap())
                            .take(size.try_into().unwrap())
                            .collect();

                        match data {
                            Ok(data) => {
                                // respond with the requested data
                                reply.data(&data);
                            }
                            Err(e) => reply.error(e.raw_os_error().unwrap()),
                        }
                    }
                }
            }
        }
    }

    #[tracing::instrument(skip_all, fields(rq.inode = ino))]
    fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: fuser::ReplyData) {
        if ino == fuser::FUSE_ROOT_ID {
            reply.error(libc::ENOSYS);
            return;
        }

        // lookup the inode
        match *self.inode_tracker.get(ino).unwrap() {
            InodeData::Directory(..) | InodeData::Regular(..) => {
                reply.error(libc::EINVAL);
            }
            InodeData::Symlink(ref target) => reply.data(target.as_bytes()),
        }
    }
}

fn reply_with_entry(reply: fuser::ReplyEntry, file_attr: &FileAttr) {
    reply.entry(&Duration::MAX, file_attr, 1 /* TODO: generation */);
}
