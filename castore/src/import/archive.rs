#[cfg(target_family = "unix")]
use std::os::unix::ffi::OsStrExt;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use tokio::io::AsyncRead;
use tokio_stream::StreamExt;
use tokio_tar::Archive;
use tracing::{instrument, Level};

use crate::{
    blobservice::BlobService,
    directoryservice::{DirectoryPutter, DirectoryService},
    import::Error,
    proto::{node::Node, Directory, DirectoryNode, FileNode, SymlinkNode},
};

/// Ingests elements from the given tar [`Archive`] into a the passed [`BlobService`] and
/// [`DirectoryService`].
#[instrument(skip_all, ret(level = Level::TRACE), err)]
pub async fn ingest_archive<'a, BS, DS, R>(
    blob_service: BS,
    directory_service: DS,
    mut archive: Archive<R>,
) -> Result<Node, Error>
where
    BS: AsRef<dyn BlobService> + Clone,
    DS: AsRef<dyn DirectoryService>,
    R: AsyncRead + Unpin,
{
    // Since tarballs can have entries in any arbitrary order, we need to
    // buffer all of the directory metadata so we can reorder directory
    // contents and entries to meet the requires of the castore.

    // In the first phase, collect up all the regular files and symlinks.
    let mut paths = HashMap::new();
    let mut entries = archive.entries().map_err(Error::Archive)?;
    while let Some(mut entry) = entries.try_next().await.map_err(Error::Archive)? {
        let path = entry.path().map_err(Error::Archive)?.into_owned();
        let name = path
            .file_name()
            .ok_or_else(|| {
                Error::Archive(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid filename in archive",
                ))
            })?
            .as_bytes()
            .to_vec()
            .into();

        let node = match entry.header().entry_type() {
            tokio_tar::EntryType::Regular
            | tokio_tar::EntryType::GNUSparse
            | tokio_tar::EntryType::Continuous => {
                // TODO: If the same path is overwritten in the tarball, we may leave
                // an unreferenced blob after uploading.
                let mut writer = blob_service.as_ref().open_write().await;
                let size = tokio::io::copy(&mut entry, &mut writer)
                    .await
                    .map_err(Error::Archive)?;
                let digest = writer.close().await.map_err(Error::Archive)?;
                Node::File(FileNode {
                    name,
                    digest: digest.into(),
                    size,
                    executable: entry.header().mode().map_err(Error::Archive)? & 64 != 0,
                })
            }
            tokio_tar::EntryType::Symlink => Node::Symlink(SymlinkNode {
                name,
                target: entry
                    .link_name()
                    .map_err(Error::Archive)?
                    .expect("symlink missing target")
                    .as_os_str()
                    .as_bytes()
                    .to_vec()
                    .into(),
            }),
            // Push a bogus directory marker so we can make sure this directoy gets
            // created. We don't know the digest and size until after reading the full
            // tarball.
            tokio_tar::EntryType::Directory => Node::Directory(DirectoryNode {
                name,
                digest: Default::default(),
                size: 0,
            }),

            tokio_tar::EntryType::XGlobalHeader | tokio_tar::EntryType::XHeader => continue,

            entry_type => return Err(Error::UnsupportedTarEntry(path, entry_type)),
        };

        paths.insert(path, node);
    }

    // In the second phase, construct all of the directories.

    // Collect into a list and then sort so all entries in the same directory
    // are next to each other.
    // We can detect boundaries between each directories to determine
    // when to construct or push directory entries.
    let mut ordered_paths = paths.into_iter().collect::<Vec<_>>();
    ordered_paths.sort_by(|a, b| a.0.cmp(&b.0));

    let mut directory_putter = directory_service.as_ref().put_multiple_start();

    // Start with an initial directory at the root.
    let mut dir_stack = vec![(PathBuf::from(""), Directory::default())];

    async fn pop_directory(
        dir_stack: &mut Vec<(PathBuf, Directory)>,
        directory_putter: &mut Box<dyn DirectoryPutter>,
    ) -> Result<DirectoryNode, Error> {
        let (path, directory) = dir_stack.pop().unwrap();

        directory
            .validate()
            .map_err(|e| Error::InvalidDirectory(path.to_path_buf(), e))?;

        let dir_node = DirectoryNode {
            name: path
                .file_name()
                .unwrap_or_default()
                .as_bytes()
                .to_vec()
                .into(),
            digest: directory.digest().into(),
            size: directory.size(),
        };

        if let Some((_, parent)) = dir_stack.last_mut() {
            parent.directories.push(dir_node.clone());
        }

        directory_putter.put(directory).await?;

        Ok(dir_node)
    }

    fn push_directories(path: &Path, dir_stack: &mut Vec<(PathBuf, Directory)>) {
        if path == dir_stack.last().unwrap().0 {
            return;
        }
        if let Some(parent) = path.parent() {
            push_directories(parent, dir_stack);
        }
        dir_stack.push((path.to_path_buf(), Directory::default()));
    }

    for (path, node) in ordered_paths.into_iter() {
        // Pop stack until the top dir is an ancestor of this entry.
        loop {
            let top = dir_stack.last().unwrap();
            if path.ancestors().any(|ancestor| ancestor == top.0) {
                break;
            }

            pop_directory(&mut dir_stack, &mut directory_putter).await?;
        }

        // For directories, just ensure the directory node exists.
        if let Node::Directory(_) = node {
            push_directories(&path, &mut dir_stack);
            continue;
        }

        // Push all ancestor directories onto the stack.
        push_directories(path.parent().unwrap(), &mut dir_stack);

        let top = dir_stack.last_mut().unwrap();
        debug_assert_eq!(Some(top.0.as_path()), path.parent());

        match node {
            Node::File(n) => top.1.files.push(n),
            Node::Symlink(n) => top.1.symlinks.push(n),
            // We already handled directories above.
            Node::Directory(_) => unreachable!(),
        }
    }

    let mut root_node = None;
    while !dir_stack.is_empty() {
        // If the root directory only has 1 directory entry, we return the child entry
        // instead... weeeee
        if dir_stack.len() == 1 && dir_stack.last().unwrap().1.directories.len() == 1 {
            break;
        }
        root_node = Some(pop_directory(&mut dir_stack, &mut directory_putter).await?);
    }
    let root_node = root_node.expect("no root node");

    let root_digest = directory_putter.close().await?;

    debug_assert_eq!(root_digest.as_slice(), &root_node.digest);

    Ok(Node::Directory(root_node))
}
