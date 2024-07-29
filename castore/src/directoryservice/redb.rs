use futures::stream::BoxStream;
use prost::Message;
use redb::{Database, TableDefinition};
use std::{path::PathBuf, sync::Arc};
use tonic::async_trait;
use tracing::{instrument, warn};

use super::{
    traverse_directory, Directory, DirectoryGraph, DirectoryPutter, DirectoryService,
    LeavesToRootValidator,
};
use crate::{
    composition::{CompositionContext, ServiceBuilder},
    digests, proto, B3Digest, Error,
};

const DIRECTORY_TABLE: TableDefinition<[u8; digests::B3_LEN], Vec<u8>> =
    TableDefinition::new("directory");

#[derive(Clone)]
pub struct RedbDirectoryService {
    // We wrap the db in an Arc to be able to move it into spawn_blocking,
    // as discussed in https://github.com/cberner/redb/issues/789
    db: Arc<Database>,
}

impl RedbDirectoryService {
    /// Constructs a new instance using the specified filesystem path for
    /// storage.
    pub async fn new(path: PathBuf) -> Result<Self, Error> {
        if path == PathBuf::from("/") {
            return Err(Error::StorageError(
                "cowardly refusing to open / with redb".to_string(),
            ));
        }

        let db = tokio::task::spawn_blocking(|| -> Result<_, redb::Error> {
            let db = redb::Database::create(path)?;
            create_schema(&db)?;
            Ok(db)
        })
        .await??;

        Ok(Self { db: Arc::new(db) })
    }

    /// Constructs a new instance using the in-memory backend.
    pub fn new_temporary() -> Result<Self, Error> {
        let db =
            redb::Database::builder().create_with_backend(redb::backends::InMemoryBackend::new())?;

        create_schema(&db)?;

        Ok(Self { db: Arc::new(db) })
    }
}

/// Ensures all tables are present.
/// Opens a write transaction and calls open_table on DIRECTORY_TABLE, which will
/// create it if not present.
fn create_schema(db: &redb::Database) -> Result<(), redb::Error> {
    let txn = db.begin_write()?;
    txn.open_table(DIRECTORY_TABLE)?;
    txn.commit()?;

    Ok(())
}

#[async_trait]
impl DirectoryService for RedbDirectoryService {
    #[instrument(skip(self, digest), fields(directory.digest = %digest))]
    async fn get(&self, digest: &B3Digest) -> Result<Option<Directory>, Error> {
        let db = self.db.clone();

        // Retrieves the protobuf-encoded Directory for the corresponding digest.
        let db_get_resp = tokio::task::spawn_blocking({
            let digest_as_array: [u8; digests::B3_LEN] = digest.to_owned().into();
            move || -> Result<_, redb::Error> {
                let txn = db.begin_read()?;
                let table = txn.open_table(DIRECTORY_TABLE)?;
                Ok(table.get(digest_as_array)?)
            }
        })
        .await?
        .map_err(|e| {
            warn!(err=%e, "failed to retrieve Directory");
            Error::StorageError("failed to retrieve Directory".to_string())
        })?;

        // The Directory was not found, return None.
        let directory_data = match db_get_resp {
            None => return Ok(None),
            Some(d) => d,
        };

        // We check that the digest of the retrieved Directory matches the expected digest.
        let actual_digest = blake3::hash(directory_data.value().as_slice());
        if actual_digest.as_bytes() != digest.as_slice() {
            warn!(directory.actual_digest=%actual_digest, "requested Directory got the wrong digest");
            return Err(Error::StorageError(
                "requested Directory got the wrong digest".to_string(),
            ));
        }

        // Attempt to decode the retrieved protobuf-encoded Directory, returning a parsing error if
        // the decoding failed.
        let directory = match proto::Directory::decode(&*directory_data.value()) {
            Ok(dir) => {
                // The returned Directory must be valid.
                dir.try_into().map_err(|e| {
                    warn!(err=%e, "Directory failed validation");
                    Error::StorageError("Directory failed validation".to_string())
                })?
            }
            Err(e) => {
                warn!(err=%e, "failed to parse Directory");
                return Err(Error::StorageError("failed to parse Directory".to_string()));
            }
        };

        Ok(Some(directory))
    }

    #[instrument(skip(self, directory), fields(directory.digest = %directory.digest()))]
    async fn put(&self, directory: Directory) -> Result<B3Digest, Error> {
        tokio::task::spawn_blocking({
            let db = self.db.clone();
            move || {
                let digest = directory.digest();

                // Store the directory in the table.
                let txn = db.begin_write()?;
                {
                    let mut table = txn.open_table(DIRECTORY_TABLE)?;
                    let digest_as_array: [u8; digests::B3_LEN] = digest.clone().into();
                    table.insert(
                        digest_as_array,
                        proto::Directory::from(directory).encode_to_vec(),
                    )?;
                }
                txn.commit()?;

                Ok(digest)
            }
        })
        .await?
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> BoxStream<'static, Result<Directory, Error>> {
        // FUTUREWORK: Ideally we should have all of the directory traversing happen in a single
        // redb transaction to avoid constantly closing and opening new transactions for the
        // database.
        traverse_directory(self.clone(), root_directory_digest)
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<dyn DirectoryPutter> {
        Box::new(RedbDirectoryPutter {
            db: self.db.clone(),
            directory_validator: Some(Default::default()),
        })
    }
}

pub struct RedbDirectoryPutter {
    db: Arc<Database>,

    /// The directories (inside the directory validator) that we insert later,
    /// or None, if they were already inserted.
    directory_validator: Option<DirectoryGraph<LeavesToRootValidator>>,
}

#[async_trait]
impl DirectoryPutter for RedbDirectoryPutter {
    #[instrument(level = "trace", skip_all, fields(directory.digest=%directory.digest()), err)]
    async fn put(&mut self, directory: Directory) -> Result<(), Error> {
        match self.directory_validator {
            None => return Err(Error::StorageError("already closed".to_string())),
            Some(ref mut validator) => {
                validator
                    .add(directory)
                    .map_err(|e| Error::StorageError(e.to_string()))?;
            }
        }

        Ok(())
    }

    #[instrument(level = "trace", skip_all, ret, err)]
    async fn close(&mut self) -> Result<B3Digest, Error> {
        match self.directory_validator.take() {
            None => Err(Error::StorageError("already closed".to_string())),
            Some(validator) => {
                // Insert all directories as a batch.
                tokio::task::spawn_blocking({
                    let txn = self.db.begin_write()?;
                    move || {
                        // Retrieve the validated directories.
                        let directories = validator
                            .validate()
                            .map_err(|e| Error::StorageError(e.to_string()))?
                            .drain_leaves_to_root()
                            .collect::<Vec<_>>();

                        // Get the root digest, which is at the end (cf. insertion order)
                        let root_digest = directories
                            .last()
                            .ok_or_else(|| Error::StorageError("got no directories".to_string()))?
                            .digest();

                        {
                            let mut table = txn.open_table(DIRECTORY_TABLE)?;

                            // Looping over all the verified directories, queuing them up for a
                            // batch insertion.
                            for directory in directories {
                                let digest_as_array: [u8; digests::B3_LEN] =
                                    directory.digest().into();
                                table.insert(
                                    digest_as_array,
                                    proto::Directory::from(directory).encode_to_vec(),
                                )?;
                            }
                        }

                        txn.commit()?;

                        Ok(root_digest)
                    }
                })
                .await?
            }
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedbDirectoryServiceConfig {
    is_temporary: bool,
    #[serde(default)]
    /// required when is_temporary = false
    path: Option<PathBuf>,
}

impl TryFrom<url::Url> for RedbDirectoryServiceConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        // redb doesn't support host, and a path can be provided (otherwise
        // it'll live in memory only).
        if url.has_host() {
            return Err(Error::StorageError("no host allowed".to_string()).into());
        }

        Ok(if url.path().is_empty() {
            RedbDirectoryServiceConfig {
                is_temporary: true,
                path: None,
            }
        } else {
            RedbDirectoryServiceConfig {
                is_temporary: false,
                path: Some(url.path().into()),
            }
        })
    }
}

#[async_trait]
impl ServiceBuilder for RedbDirectoryServiceConfig {
    type Output = dyn DirectoryService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        _context: &CompositionContext,
    ) -> Result<Arc<dyn DirectoryService>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        match self {
            RedbDirectoryServiceConfig {
                is_temporary: true,
                path: None,
            } => Ok(Arc::new(RedbDirectoryService::new_temporary()?)),
            RedbDirectoryServiceConfig {
                is_temporary: true,
                path: Some(_),
            } => Err(Error::StorageError(
                "Temporary RedbDirectoryService can not have path".into(),
            )
            .into()),
            RedbDirectoryServiceConfig {
                is_temporary: false,
                path: None,
            } => Err(Error::StorageError("RedbDirectoryService is missing path".into()).into()),
            RedbDirectoryServiceConfig {
                is_temporary: false,
                path: Some(path),
            } => Ok(Arc::new(RedbDirectoryService::new(path.into()).await?)),
        }
    }
}
