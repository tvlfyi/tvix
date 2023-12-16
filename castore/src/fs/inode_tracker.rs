use std::{collections::HashMap, sync::Arc};

use super::inodes::{DirectoryInodeData, InodeData};
use crate::B3Digest;

/// InodeTracker keeps track of inodes, stores data being these inodes and deals
/// with inode allocation.
pub struct InodeTracker {
    data: HashMap<u64, Arc<InodeData>>,

    // lookup table for blobs by their B3Digest
    blob_digest_to_inode: HashMap<B3Digest, u64>,

    // lookup table for symlinks by their target
    symlink_target_to_inode: HashMap<bytes::Bytes, u64>,

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

    // Replaces data for a given inode.
    // Panics if the inode doesn't already exist.
    pub fn replace(&mut self, ino: u64, data: Arc<InodeData>) {
        if self.data.insert(ino, data).is_none() {
            panic!("replace called on unknown inode");
        }
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
            // Inserting [DirectoryInodeData::Populated] doesn't normally happen,
            // only via [replace].
            InodeData::Directory(DirectoryInodeData::Populated(..)) => {
                unreachable!("should never be called with DirectoryInodeData::Populated")
            }
        }
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
                self.symlink_target_to_inode.insert(target.clone(), ino);
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
    use crate::fixtures;

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
            fixtures::BLOB_A.len() as u64,
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
                fixtures::BLOB_B.len() as u64,
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
}
