use super::PathInfoService;
use crate::proto::PathInfo;
use data_encoding::BASE64;
use futures::{stream::BoxStream, StreamExt};
use prost::Message;
use redb::{Database, ReadableTable, TableDefinition};
use std::{path::PathBuf, sync::Arc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::async_trait;
use tracing::{instrument, warn};
use tvix_castore::{
    composition::{CompositionContext, ServiceBuilder},
    Error,
};

const PATHINFO_TABLE: TableDefinition<[u8; 20], Vec<u8>> = TableDefinition::new("pathinfo");

/// PathInfoService implementation using redb under the hood.
/// redb stores all of its data in a single file with a K/V pointing from a path's output hash to
/// its corresponding protobuf-encoded PathInfo.
pub struct RedbPathInfoService {
    // We wrap db in an Arc to be able to move it into spawn_blocking,
    // as discussed in https://github.com/cberner/redb/issues/789
    db: Arc<Database>,
}

impl RedbPathInfoService {
    /// Constructs a new instance using the specified file system path for
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
/// Opens a write transaction and calls open_table on PATHINFO_TABLE, which will
/// create it if not present.
fn create_schema(db: &redb::Database) -> Result<(), redb::Error> {
    let txn = db.begin_write()?;
    txn.open_table(PATHINFO_TABLE)?;
    txn.commit()?;

    Ok(())
}

#[async_trait]
impl PathInfoService for RedbPathInfoService {
    #[instrument(level = "trace", skip_all, fields(path_info.digest = BASE64.encode(&digest)))]
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        let db = self.db.clone();

        tokio::task::spawn_blocking({
            move || {
                let txn = db.begin_read()?;
                let table = txn.open_table(PATHINFO_TABLE)?;
                match table.get(digest)? {
                    Some(pathinfo_bytes) => Ok(Some(
                        PathInfo::decode(pathinfo_bytes.value().as_slice()).map_err(|e| {
                            warn!(err=%e, "failed to decode stored PathInfo");
                            Error::StorageError("failed to decode stored PathInfo".to_string())
                        })?,
                    )),
                    None => Ok(None),
                }
            }
        })
        .await?
    }

    #[instrument(level = "trace", skip_all, fields(path_info.root_node = ?path_info.node))]
    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error> {
        // Call validate on the received PathInfo message.
        let store_path = path_info
            .validate()
            .map_err(|e| {
                warn!(err=%e, "failed to validate PathInfo");
                Error::StorageError("failed to validate PathInfo".to_string())
            })?
            .to_owned();

        let path_info_encoded = path_info.encode_to_vec();
        let db = self.db.clone();

        tokio::task::spawn_blocking({
            move || -> Result<(), Error> {
                let txn = db.begin_write()?;
                {
                    let mut table = txn.open_table(PATHINFO_TABLE)?;
                    table
                        .insert(store_path.digest(), path_info_encoded)
                        .map_err(|e| {
                            warn!(err=%e, "failed to insert PathInfo");
                            Error::StorageError("failed to insert PathInfo".to_string())
                        })?;
                }
                Ok(txn.commit()?)
            }
        })
        .await??;

        Ok(path_info)
    }

    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
        let db = self.db.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(50);

        // Spawn a blocking task which writes all PathInfos to tx.
        tokio::task::spawn_blocking({
            move || -> Result<(), Error> {
                let read_txn = db.begin_read()?;
                let table = read_txn.open_table(PATHINFO_TABLE)?;

                for elem in table.iter()? {
                    let elem = elem?;
                    tokio::runtime::Handle::current()
                        .block_on(tx.send(Ok(
                            PathInfo::decode(elem.1.value().as_slice()).map_err(|e| {
                                warn!(err=%e, "invalid PathInfo");
                                Error::StorageError("invalid PathInfo".to_string())
                            })?,
                        )))
                        .map_err(|e| Error::StorageError(e.to_string()))?;
                }

                Ok(())
            }
        });

        ReceiverStream::from(rx).boxed()
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedbPathInfoServiceConfig {
    is_temporary: bool,
    #[serde(default)]
    /// required when is_temporary = false
    path: Option<PathBuf>,
}

impl TryFrom<url::Url> for RedbPathInfoServiceConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        // redb doesn't support host, and a path can be provided (otherwise it'll live in memory only)
        if url.has_host() {
            return Err(Error::StorageError("no host allowed".to_string()).into());
        }

        Ok(if url.path().is_empty() {
            RedbPathInfoServiceConfig {
                is_temporary: true,
                path: None,
            }
        } else {
            RedbPathInfoServiceConfig {
                is_temporary: false,
                path: Some(url.path().into()),
            }
        })
    }
}

#[async_trait]
impl ServiceBuilder for RedbPathInfoServiceConfig {
    type Output = dyn PathInfoService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        _context: &CompositionContext,
    ) -> Result<Arc<dyn PathInfoService>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        match self {
            RedbPathInfoServiceConfig {
                is_temporary: true,
                path: None,
            } => Ok(Arc::new(RedbPathInfoService::new_temporary()?)),
            RedbPathInfoServiceConfig {
                is_temporary: true,
                path: Some(_),
            } => Err(
                Error::StorageError("Temporary RedbPathInfoService can not have path".into())
                    .into(),
            ),
            RedbPathInfoServiceConfig {
                is_temporary: false,
                path: None,
            } => Err(Error::StorageError("RedbPathInfoService is missing path".into()).into()),
            RedbPathInfoServiceConfig {
                is_temporary: false,
                path: Some(path),
            } => Ok(Arc::new(RedbPathInfoService::new(path.to_owned()).await?)),
        }
    }
}
