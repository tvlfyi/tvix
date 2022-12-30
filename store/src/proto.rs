#![allow(clippy::derive_partial_eq_without_eq)]
// https://github.com/hyperium/tonic/issues/1056
use std::collections::HashSet;
use thiserror::Error;

use prost::Message;

tonic::include_proto!("tvix.store.v1");

/// Errors that can occur during the validation of Directory messages.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum ValidateDirectoryError {
    /// Elements are not in sorted order
    #[error("{0} is not sorted")]
    WrongSorting(String),
    /// Multiple elements with the same name encountered
    #[error("{0} is a duplicate name")]
    DuplicateName(String),
    /// Invalid name encountered
    #[error("Invalid name in {0}")]
    InvalidName(String),
    /// Invalid digest length encountered
    #[error("Invalid Digest length: {0}")]
    InvalidDigestLen(usize),
}

/// Checks a Node name for validity as an intermediate node, and returns an
/// error that's generated from the supplied constructor.
///
/// We disallow slashes, null bytes, '.', '..' and the empty string.
fn validate_node_name<E>(name: &str, err: fn(String) -> E) -> Result<(), E> {
    if name.is_empty() || name == ".." || name == "." || name.contains('\x00') || name.contains('/')
    {
        return Err(err(name.to_string()));
    }
    Ok(())
}

/// Checks a digest for validity.
/// Digests are 32 bytes long, as we store blake3 digests.
fn validate_digest<E>(digest: &Vec<u8>, err: fn(usize) -> E) -> Result<(), E> {
    if digest.len() != 32 {
        return Err(err(digest.len()));
    }
    Ok(())
}

/// Accepts a name, and a mutable reference to the previous name.
/// If the passed name is larger than the previous one, the reference is updated.
/// If it's not, an error is returned.
fn update_if_lt_prev<'set, 'n>(
    prev_name: &'set mut &'n str,
    name: &'n str,
) -> Result<(), ValidateDirectoryError> {
    if *name < **prev_name {
        return Err(ValidateDirectoryError::WrongSorting(name.to_string()));
    }
    *prev_name = name;
    Ok(())
}

/// Inserts the given name into a HashSet if it's not already in there.
/// If it is, an error is returned.
fn insert_once<'n>(
    seen_names: &mut HashSet<&'n str>,
    name: &'n str,
) -> Result<(), ValidateDirectoryError> {
    if seen_names.get(name).is_some() {
        return Err(ValidateDirectoryError::DuplicateName(name.to_string()));
    }
    seen_names.insert(name);
    Ok(())
}

impl Directory {
    // The size of a directory is the number of all regular and symlink elements,
    // the number of directory elements, and their size fields.
    pub fn size(&self) -> u32 {
        self.files.len() as u32
            + self.symlinks.len() as u32
            + self
                .directories
                .iter()
                .fold(0, |acc: u32, e| (acc + 1 + e.size) as u32)
    }

    pub fn digest(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();

        hasher.update(&self.encode_to_vec()).finalize().as_bytes()[..].to_vec()
    }

    /// validate checks the directory for invalid data, such as:
    /// - violations of name restrictions
    /// - invalid digest lengths
    /// - not properly sorted lists
    /// - duplicate names in the three lists
    pub fn validate(&self) -> Result<(), ValidateDirectoryError> {
        let mut seen_names: HashSet<&str> = HashSet::new();

        let mut last_directory_name: &str = "";
        let mut last_file_name: &str = "";
        let mut last_symlink_name: &str = "";

        // check directories
        for directory_node in &self.directories {
            validate_node_name(&directory_node.name, ValidateDirectoryError::InvalidName)?;
            validate_digest(
                &directory_node.digest,
                ValidateDirectoryError::InvalidDigestLen,
            )?;

            update_if_lt_prev(&mut last_directory_name, directory_node.name.as_str())?;
            insert_once(&mut seen_names, directory_node.name.as_str())?;
        }

        // check files
        for file_node in &self.files {
            validate_node_name(&file_node.name, ValidateDirectoryError::InvalidName)?;
            validate_digest(&file_node.digest, ValidateDirectoryError::InvalidDigestLen)?;

            update_if_lt_prev(&mut last_file_name, file_node.name.as_str())?;
            insert_once(&mut seen_names, file_node.name.as_str())?;
        }

        // check symlinks
        for symlink_node in &self.symlinks {
            validate_node_name(&symlink_node.name, ValidateDirectoryError::InvalidName)?;

            update_if_lt_prev(&mut last_symlink_name, symlink_node.name.as_str())?;
            insert_once(&mut seen_names, symlink_node.name.as_str())?;
        }

        Ok(())
    }
}
