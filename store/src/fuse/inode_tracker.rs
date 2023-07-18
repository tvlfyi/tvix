use std::{collections::HashMap, sync::Arc};

use crate::{proto, B3Digest};

use super::inodes::{DirectoryInodeData, InodeData};

/// InodeTracker keeps track of inodes, stores data being these inodes and deals
/// with inode allocation.
pub struct InodeTracker {
    data: HashMap<u64, Arc<InodeData>>,

    // lookup table for blobs by their B3Digest
    blob_digest_to_inode: HashMap<B3Digest, u64>,

    // lookup table for symlinks by their target
    symlink_target_to_inode: HashMap<Vec<u8>, u64>,

    // lookup table for directories by their B3Digest.
    // Note the corresponding directory may not be present in data yet.
    directory_digest_to_inode: HashMap<B3Digest, u64>,

    // the next inode to allocate
    next_inode: u64,
}

impl Default for InodeTracker {
    fn default() -> Self {
        Self {
            data: Default::default(),

            blob_digest_to_inode: Default::default(),
            symlink_target_to_inode: Default::default(),
            directory_digest_to_inode: Default::default(),

            next_inode: 2,
        }
    }
}

impl InodeTracker {
    // Retrieves data for a given inode, if it exists.
    pub fn get(&self, ino: u64) -> Option<Arc<InodeData>> {
        self.data.get(&ino).cloned()
    }

    // Stores data and returns the inode for it.
    // In case an inode has already been allocated for the same data, that inode
    // is returned, otherwise a new one is allocated.
    // In case data is a [InodeData::Directory], inodes for all items are looked
    // up
    pub fn put(&mut self, data: InodeData) -> u64 {
        match data {
            InodeData::Regular(ref digest, _, _) => {
                match self.blob_digest_to_inode.get(digest) {
                    Some(found_ino) => {
                        // We already have it, return the inode.
                        *found_ino
                    }
                    None => self.insert_and_increment(data),
                }
            }
            InodeData::Symlink(ref target) => {
                match self.symlink_target_to_inode.get(target) {
                    Some(found_ino) => {
                        // We already have it, return the inode.
                        *found_ino
                    }
                    None => self.insert_and_increment(data),
                }
            }
            InodeData::Directory(DirectoryInodeData::Sparse(ref digest, _size)) => {
                // check the lookup table if the B3Digest is known.
                match self.directory_digest_to_inode.get(digest) {
                    Some(found_ino) => {
                        // We already have it, return the inode.
                        *found_ino
                    }
                    None => {
                        // insert and return the inode
                        self.insert_and_increment(data)
                    }
                }
            }
            // Inserting [DirectoryInodeData::Populated] usually replaces an
            // existing [DirectoryInodeData::Sparse] one.
            InodeData::Directory(DirectoryInodeData::Populated(ref digest, ref children)) => {
                let dir_ino = self.directory_digest_to_inode.get(digest);
                if let Some(dir_ino) = dir_ino {
                    let dir_ino = *dir_ino;

                    // We know the data must exist, as we found it in [directory_digest_to_inode].
                    let needs_update = match **self.data.get(&dir_ino).unwrap() {
                        InodeData::Regular(..) | InodeData::Symlink(_) => {
                            panic!("unexpected type at inode {}", dir_ino);
                        }
                        // already populated, nothing to do
                        InodeData::Directory(DirectoryInodeData::Populated(..)) => false,
                        // in case the actual data is sparse, replace it with the populated one.
                        // this allocates inodes for new children in the process.
                        InodeData::Directory(DirectoryInodeData::Sparse(
                            ref old_digest,
                            ref _old_size,
                        )) => {
                            // sanity checking to ensure we update the right node
                            debug_assert_eq!(old_digest, digest);

                            true
                        }
                    };

                    if needs_update {
                        // populate inode fields in children
                        let children = self.allocate_inodes_for_children(children.to_vec());

                        // update sparse data with populated data
                        self.data.insert(
                            dir_ino,
                            Arc::new(InodeData::Directory(DirectoryInodeData::Populated(
                                digest.clone(),
                                children,
                            ))),
                        );
                    }

                    dir_ino
                } else {
                    // populate inode fields in children
                    let children = self.allocate_inodes_for_children(children.to_vec());
                    // insert and return InodeData
                    self.insert_and_increment(InodeData::Directory(DirectoryInodeData::Populated(
                        digest.clone(),
                        children,
                    )))
                }
            }
        }
    }

    // Consume a list of children with zeroed inodes, and allocate (or fetch existing) inodes.
    fn allocate_inodes_for_children(
        &mut self,
        children: Vec<(u64, proto::node::Node)>,
    ) -> Vec<(u64, proto::node::Node)> {
        // allocate new inodes for all children
        let mut children_new: Vec<(u64, proto::node::Node)> = Vec::new();

        for (child_ino, ref child_node) in children {
            debug_assert_eq!(0, child_ino, "expected child inode to be 0");
            let child_ino = match child_node {
                proto::node::Node::Directory(directory_node) => {
                    // Try putting the sparse data in. If we already have a
                    // populated version, it'll not update it.
                    self.put(directory_node.into())
                }
                proto::node::Node::File(file_node) => self.put(file_node.into()),
                proto::node::Node::Symlink(symlink_node) => self.put(symlink_node.into()),
            };

            children_new.push((child_ino, child_node.clone()))
        }
        children_new
    }

    // Inserts the data and returns the inode it was stored at, while
    // incrementing next_inode.
    fn insert_and_increment(&mut self, data: InodeData) -> u64 {
        let ino = self.next_inode;
        // insert into lookup tables
        match data {
            InodeData::Regular(ref digest, _, _) => {
                self.blob_digest_to_inode.insert(digest.clone(), ino);
            }
            InodeData::Symlink(ref target) => {
                self.symlink_target_to_inode.insert(target.to_vec(), ino);
            }
            InodeData::Directory(DirectoryInodeData::Sparse(ref digest, _size)) => {
                self.directory_digest_to_inode.insert(digest.clone(), ino);
            }
            // This is currently not used outside test fixtures.
            // Usually a [DirectoryInodeData::Sparse] is inserted and later
            // "upgraded" with more data.
            // However, as a future optimization, a lookup for a PathInfo could trigger a
            // [DirectoryService::get_recursive()] request that "forks into
            // background" and prepopulates all Directories in a closure.
            InodeData::Directory(DirectoryInodeData::Populated(ref digest, _)) => {
                self.directory_digest_to_inode.insert(digest.clone(), ino);
            }
        }
        // Insert data
        self.data.insert(ino, Arc::new(data));

        // increment inode counter and return old inode.
        self.next_inode += 1;
        ino
    }
}

#[cfg(test)]
mod tests {
    use crate::fuse::inodes::DirectoryInodeData;
    use crate::proto;
    use crate::tests::fixtures;

    use super::InodeData;
    use super::InodeTracker;

    /// Getting something non-existent should be none
    #[test]
    fn get_nonexistent() {
        let inode_tracker = InodeTracker::default();
        assert!(inode_tracker.get(1).is_none());
    }

    /// Put of a regular file should allocate a uid, which should be the same when inserting again.
    #[test]
    fn put_regular() {
        let mut inode_tracker = InodeTracker::default();
        let f = InodeData::Regular(
            fixtures::BLOB_A_DIGEST.clone(),
            fixtures::BLOB_A.len() as u32,
            false,
        );

        // put it in
        let ino = inode_tracker.put(f.clone());

        // a get should return the right data
        let data = inode_tracker.get(ino).expect("must be some");
        match *data {
            InodeData::Regular(ref digest, _, _) => {
                assert_eq!(&fixtures::BLOB_A_DIGEST.clone(), digest);
            }
            InodeData::Symlink(_) | InodeData::Directory(..) => panic!("wrong type"),
        }

        // another put should return the same ino
        assert_eq!(ino, inode_tracker.put(f));

        // inserting another file should return a different ino
        assert_ne!(
            ino,
            inode_tracker.put(InodeData::Regular(
                fixtures::BLOB_B_DIGEST.clone(),
                fixtures::BLOB_B.len() as u32,
                false,
            ))
        );
    }

    // Put of a symlink should allocate a uid, which should be the same when inserting again
    #[test]
    fn put_symlink() {
        let mut inode_tracker = InodeTracker::default();
        let f = InodeData::Symlink("target".into());

        // put it in
        let ino = inode_tracker.put(f.clone());

        // a get should return the right data
        let data = inode_tracker.get(ino).expect("must be some");
        match *data {
            InodeData::Symlink(ref target) => {
                assert_eq!(b"target".to_vec(), *target);
            }
            InodeData::Regular(..) | InodeData::Directory(..) => panic!("wrong type"),
        }

        // another put should return the same ino
        assert_eq!(ino, inode_tracker.put(f));

        // inserting another file should return a different ino
        assert_ne!(ino, inode_tracker.put(InodeData::Symlink("target2".into())));
    }

    // TODO: put sparse directory

    /// Put a directory into the inode tracker, which refers to a file not seen yet.
    #[test]
    fn put_directory_leaf() {
        let mut inode_tracker = InodeTracker::default();

        // this is a directory with a single item, a ".keep" file pointing to a 0 bytes blob.
        let dir: InodeData = fixtures::DIRECTORY_WITH_KEEP.clone().into();

        // put it in
        let dir_ino = inode_tracker.put(dir.clone());

        // a get should return the right data
        let data = inode_tracker.get(dir_ino).expect("must be some");
        match *data {
            InodeData::Directory(super::DirectoryInodeData::Sparse(..)) => {
                panic!("wrong type");
            }
            InodeData::Directory(super::DirectoryInodeData::Populated(
                ref directory_digest,
                ref children,
            )) => {
                // ensure the directory digest matches
                assert_eq!(&fixtures::DIRECTORY_WITH_KEEP.digest(), directory_digest);

                // ensure the child is populated, with a different inode than
                // the parent, and the data matches expectations.
                assert_eq!(1, children.len());
                let (child_ino, child_node) = children.first().unwrap();
                assert_ne!(dir_ino, *child_ino);
                assert_eq!(
                    &proto::node::Node::File(
                        fixtures::DIRECTORY_WITH_KEEP.files.first().unwrap().clone()
                    ),
                    child_node
                );

                // ensure looking up that inode directly returns the data
                let child_data = inode_tracker.get(*child_ino).expect("must exist");
                match *child_data {
                    InodeData::Regular(ref digest, size, executable) => {
                        assert_eq!(&fixtures::EMPTY_BLOB_DIGEST.clone(), digest);
                        assert_eq!(0, size);
                        assert_eq!(false, executable);
                    }
                    InodeData::Symlink(_) | InodeData::Directory(..) => panic!("wrong type"),
                }
            }
            InodeData::Symlink(_) | InodeData::Regular(..) => panic!("wrong type"),
        }
    }

    /// Put a directory into the inode tracker, referring to files, directories
    /// and symlinks not seen yet.
    #[test]
    fn put_directory_complicated() {
        let mut inode_tracker = InodeTracker::default();

        // this is a directory with a single item, a ".keep" file pointing to a 0 bytes blob.
        let dir_complicated: InodeData = fixtures::DIRECTORY_COMPLICATED.clone().into();

        // put it in
        let dir_complicated_ino = inode_tracker.put(dir_complicated.clone());

        // a get should return the right data
        let dir_data = inode_tracker
            .get(dir_complicated_ino)
            .expect("must be some");

        let child_dir_ino = match *dir_data {
            InodeData::Directory(DirectoryInodeData::Sparse(..)) => {
                panic!("wrong type");
            }
            InodeData::Directory(DirectoryInodeData::Populated(
                ref directory_digest,
                ref children,
            )) => {
                // assert the directory digest matches
                assert_eq!(&fixtures::DIRECTORY_COMPLICATED.digest(), directory_digest);

                // ensure there's three children, all with different inodes
                assert_eq!(3, children.len());
                let mut seen_inodes = Vec::from([dir_complicated_ino]);

                // check the first child (.keep)
                {
                    let (child_ino, child_node) = &children[0];
                    assert!(!seen_inodes.contains(&child_ino));
                    assert_eq!(
                        &proto::node::Node::File(fixtures::DIRECTORY_COMPLICATED.files[0].clone()),
                        child_node
                    );
                    seen_inodes.push(*child_ino);
                }

                // check the second child (aa)
                {
                    let (child_ino, child_node) = &children[1];
                    assert!(!seen_inodes.contains(&child_ino));
                    assert_eq!(
                        &proto::node::Node::Symlink(
                            fixtures::DIRECTORY_COMPLICATED.symlinks[0].clone()
                        ),
                        child_node
                    );
                    seen_inodes.push(*child_ino);
                }

                // check the third child (keep)
                {
                    let (child_ino, child_node) = &children[2];
                    assert!(!seen_inodes.contains(&child_ino));
                    assert_eq!(
                        &proto::node::Node::Directory(
                            fixtures::DIRECTORY_COMPLICATED.directories[0].clone()
                        ),
                        child_node
                    );
                    seen_inodes.push(*child_ino);

                    // return the child_ino
                    *child_ino
                }
            }
            InodeData::Regular(..) | InodeData::Symlink(_) => panic!("wrong type"),
        };

        // get of the inode for child_ino
        let child_dir_data = inode_tracker.get(child_dir_ino).expect("must be some");
        // it should be a sparse InodeData::Directory with the right digest.
        match *child_dir_data {
            InodeData::Directory(DirectoryInodeData::Sparse(
                ref child_dir_digest,
                child_dir_size,
            )) => {
                assert_eq!(&fixtures::DIRECTORY_WITH_KEEP.digest(), child_dir_digest);
                assert_eq!(fixtures::DIRECTORY_WITH_KEEP.size(), child_dir_size);
            }
            InodeData::Directory(DirectoryInodeData::Populated(..))
            | InodeData::Regular(..)
            | InodeData::Symlink(_) => {
                panic!("wrong type")
            }
        }

        // put DIRECTORY_WITH_KEEP, which should return the same ino as [child_dir_ino],
        // but update the sparse object to a populated one at the same time.
        let child_dir_ino2 = inode_tracker.put(fixtures::DIRECTORY_WITH_KEEP.clone().into());
        assert_eq!(child_dir_ino, child_dir_ino2);

        // get the data
        match *inode_tracker.get(child_dir_ino).expect("must be some") {
            // it should be a populated InodeData::Directory with the right digest!
            InodeData::Directory(DirectoryInodeData::Populated(
                ref directory_digest,
                ref children,
            )) => {
                // ensure the directory digest matches
                assert_eq!(&fixtures::DIRECTORY_WITH_KEEP.digest(), directory_digest);

                // ensure the child is populated, with a different inode than
                // the parent, and the data matches expectations.
                assert_eq!(1, children.len());
                let (child_node_inode, child_node) = children.first().unwrap();
                assert_ne!(dir_complicated_ino, *child_node_inode);
                assert_eq!(
                    &proto::node::Node::File(
                        fixtures::DIRECTORY_WITH_KEEP.files.first().unwrap().clone()
                    ),
                    child_node
                );
            }
            InodeData::Directory(DirectoryInodeData::Sparse(..))
            | InodeData::Regular(..)
            | InodeData::Symlink(_) => panic!("wrong type"),
        }
    }
}

// TODO: add test inserting a populated one first, then ensure an update doesn't degrade it back to sparse.
