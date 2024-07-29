use bigtable_rs::{bigtable, google::bigtable::v2 as bigtable_v2};
use bytes::Bytes;
use data_encoding::HEXLOWER;
use futures::stream::BoxStream;
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};
use std::sync::Arc;
use tonic::async_trait;
use tracing::{instrument, trace, warn};

use super::{
    utils::traverse_directory, Directory, DirectoryPutter, DirectoryService, SimplePutter,
};
use crate::composition::{CompositionContext, ServiceBuilder};
use crate::{proto, B3Digest, Error};

/// There should not be more than 10 MiB in a single cell.
/// https://cloud.google.com/bigtable/docs/schema-design#cells
const CELL_SIZE_LIMIT: u64 = 10 * 1024 * 1024;

/// Provides a [DirectoryService] implementation using
/// [Bigtable](https://cloud.google.com/bigtable/docs/)
/// as an underlying K/V store.
///
/// # Data format
/// We use Bigtable as a plain K/V store.
/// The row key is the digest of the directory, in hexlower.
/// Inside the row, we currently have a single column/cell, again using the
/// hexlower directory digest.
/// Its value is the Directory message, serialized in canonical protobuf.
/// We currently only populate this column.
///
/// In the future, we might want to introduce "bucketing", essentially storing
/// all directories inserted via `put_multiple_start` in a batched form.
/// This will prevent looking up intermediate Directories, which are not
/// directly at the root, so rely on store composition.
#[derive(Clone)]
pub struct BigtableDirectoryService {
    client: bigtable::BigTable,
    params: BigtableParameters,

    #[cfg(test)]
    #[allow(dead_code)]
    /// Holds the temporary directory containing the unix socket, and the
    /// spawned emulator process.
    emulator: std::sync::Arc<(tempfile::TempDir, async_process::Child)>,
}

impl BigtableDirectoryService {
    #[cfg(not(test))]
    pub async fn connect(params: BigtableParameters) -> Result<Self, bigtable::Error> {
        let connection = bigtable::BigTableConnection::new(
            &params.project_id,
            &params.instance_name,
            params.is_read_only,
            params.channel_size,
            params.timeout,
        )
        .await?;

        Ok(Self {
            client: connection.client(),
            params,
        })
    }

    #[cfg(test)]
    pub async fn connect(params: BigtableParameters) -> Result<Self, bigtable::Error> {
        use std::time::Duration;

        use async_process::{Command, Stdio};
        use tempfile::TempDir;
        use tokio_retry::{strategy::ExponentialBackoff, Retry};

        let tmpdir = TempDir::new().unwrap();

        let socket_path = tmpdir.path().join("cbtemulator.sock");

        let emulator_process = Command::new("cbtemulator")
            .arg("-address")
            .arg(socket_path.clone())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("failed to spawn emulator");

        Retry::spawn(
            ExponentialBackoff::from_millis(20)
                .max_delay(Duration::from_secs(1))
                .take(3),
            || async {
                if socket_path.exists() {
                    Ok(())
                } else {
                    Err(())
                }
            },
        )
        .await
        .expect("failed to wait for socket");

        // populate the emulator
        for cmd in &[
            vec!["createtable", &params.table_name],
            vec!["createfamily", &params.table_name, &params.family_name],
        ] {
            Command::new("cbt")
                .args({
                    let mut args = vec![
                        "-instance",
                        &params.instance_name,
                        "-project",
                        &params.project_id,
                    ];
                    args.extend_from_slice(cmd);
                    args
                })
                .env(
                    "BIGTABLE_EMULATOR_HOST",
                    format!("unix://{}", socket_path.to_string_lossy()),
                )
                .output()
                .await
                .expect("failed to run cbt setup command");
        }

        let connection = bigtable_rs::bigtable::BigTableConnection::new_with_emulator(
            &format!("unix://{}", socket_path.to_string_lossy()),
            &params.project_id,
            &params.instance_name,
            params.is_read_only,
            params.timeout,
        )?;

        Ok(Self {
            client: connection.client(),
            params,
            emulator: (tmpdir, emulator_process).into(),
        })
    }
}

/// Derives the row/column key for a given blake3 digest.
/// We use hexlower encoding, also because it can't be misinterpreted as RE2.
fn derive_directory_key(digest: &B3Digest) -> String {
    HEXLOWER.encode(digest.as_slice())
}

#[async_trait]
impl DirectoryService for BigtableDirectoryService {
    #[instrument(skip(self, digest), err, fields(directory.digest = %digest))]
    async fn get(&self, digest: &B3Digest) -> Result<Option<Directory>, Error> {
        let mut client = self.client.clone();
        let directory_key = derive_directory_key(digest);

        let request = bigtable_v2::ReadRowsRequest {
            app_profile_id: self.params.app_profile_id.to_string(),
            table_name: client.get_full_table_name(&self.params.table_name),
            rows_limit: 1,
            rows: Some(bigtable_v2::RowSet {
                row_keys: vec![directory_key.clone().into()],
                row_ranges: vec![],
            }),
            // Filter selected family name, and column qualifier matching our digest.
            // This is to ensure we don't fail once we start bucketing.
            filter: Some(bigtable_v2::RowFilter {
                filter: Some(bigtable_v2::row_filter::Filter::Chain(
                    bigtable_v2::row_filter::Chain {
                        filters: vec![
                            bigtable_v2::RowFilter {
                                filter: Some(
                                    bigtable_v2::row_filter::Filter::FamilyNameRegexFilter(
                                        self.params.family_name.to_string(),
                                    ),
                                ),
                            },
                            bigtable_v2::RowFilter {
                                filter: Some(
                                    bigtable_v2::row_filter::Filter::ColumnQualifierRegexFilter(
                                        directory_key.clone().into(),
                                    ),
                                ),
                            },
                        ],
                    },
                )),
            }),
            ..Default::default()
        };

        let mut response = client
            .read_rows(request)
            .await
            .map_err(|e| Error::StorageError(format!("unable to read rows: {}", e)))?;

        if response.len() != 1 {
            if response.len() > 1 {
                // This shouldn't happen, we limit number of rows to 1
                return Err(Error::StorageError(
                    "got more than one row from bigtable".into(),
                ));
            }
            // else, this is simply a "not found".
            return Ok(None);
        }

        let (row_key, mut row_cells) = response.pop().unwrap();
        if row_key != directory_key.as_bytes() {
            // This shouldn't happen, we requested this row key.
            return Err(Error::StorageError(
                "got wrong row key from bigtable".into(),
            ));
        }

        let row_cell = row_cells
            .pop()
            .ok_or_else(|| Error::StorageError("found no cells".into()))?;

        // Ensure there's only one cell (so no more left after the pop())
        // This shouldn't happen, We filter out other cells in our query.
        if !row_cells.is_empty() {
            return Err(Error::StorageError(
                "more than one cell returned from bigtable".into(),
            ));
        }

        // We also require the qualifier to be correct in the filter above,
        // so this shouldn't happen.
        if directory_key.as_bytes() != row_cell.qualifier {
            return Err(Error::StorageError("unexpected cell qualifier".into()));
        }

        // For the data in that cell, ensure the digest matches what's requested, before parsing.
        let got_digest = B3Digest::from(blake3::hash(&row_cell.value).as_bytes());
        if got_digest != *digest {
            return Err(Error::StorageError(format!(
                "invalid digest: {}",
                got_digest
            )));
        }

        // Try to parse the value into a Directory message.
        let directory = proto::Directory::decode(Bytes::from(row_cell.value))
            .map_err(|e| Error::StorageError(format!("unable to decode directory proto: {}", e)))?
            .try_into()
            .map_err(|e| Error::StorageError(format!("invalid Directory message: {}", e)))?;

        Ok(Some(directory))
    }

    #[instrument(skip(self, directory), err, fields(directory.digest = %directory.digest()))]
    async fn put(&self, directory: Directory) -> Result<B3Digest, Error> {
        let directory_digest = directory.digest();
        let mut client = self.client.clone();
        let directory_key = derive_directory_key(&directory_digest);

        let data = proto::Directory::from(directory).encode_to_vec();
        if data.len() as u64 > CELL_SIZE_LIMIT {
            return Err(Error::StorageError(
                "Directory exceeds cell limit on Bigtable".into(),
            ));
        }

        let resp = client
            .check_and_mutate_row(bigtable_v2::CheckAndMutateRowRequest {
                table_name: client.get_full_table_name(&self.params.table_name),
                app_profile_id: self.params.app_profile_id.to_string(),
                row_key: directory_key.clone().into(),
                predicate_filter: Some(bigtable_v2::RowFilter {
                    filter: Some(bigtable_v2::row_filter::Filter::ColumnQualifierRegexFilter(
                        directory_key.clone().into(),
                    )),
                }),
                // If the column was already found, do nothing.
                true_mutations: vec![],
                // Else, do the insert.
                false_mutations: vec![
                    // https://cloud.google.com/bigtable/docs/writes
                    bigtable_v2::Mutation {
                        mutation: Some(bigtable_v2::mutation::Mutation::SetCell(
                            bigtable_v2::mutation::SetCell {
                                family_name: self.params.family_name.to_string(),
                                column_qualifier: directory_key.clone().into(),
                                timestamp_micros: -1, // use server time to fill timestamp
                                value: data,
                            },
                        )),
                    },
                ],
            })
            .await
            .map_err(|e| Error::StorageError(format!("unable to mutate rows: {}", e)))?;

        if resp.predicate_matched {
            trace!("already existed")
        }

        Ok(directory_digest)
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> BoxStream<'static, Result<Directory, Error>> {
        traverse_directory(self.clone(), root_directory_digest)
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<(dyn DirectoryPutter + 'static)>
    where
        Self: Clone,
    {
        Box::new(SimplePutter::new(self.clone()))
    }
}

/// Represents configuration of [BigtableDirectoryService].
/// This currently conflates both connect parameters and data model/client
/// behaviour parameters.
#[serde_as]
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BigtableParameters {
    project_id: String,
    instance_name: String,
    #[serde(default)]
    is_read_only: bool,
    #[serde(default = "default_channel_size")]
    channel_size: usize,

    #[serde_as(as = "Option<DurationSeconds<String>>")]
    #[serde(default = "default_timeout")]
    timeout: Option<std::time::Duration>,
    table_name: String,
    family_name: String,

    #[serde(default = "default_app_profile_id")]
    app_profile_id: String,
}

#[async_trait]
impl ServiceBuilder for BigtableParameters {
    type Output = dyn DirectoryService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        _context: &CompositionContext,
    ) -> Result<Arc<dyn DirectoryService>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Arc::new(
            BigtableDirectoryService::connect(self.clone()).await?,
        ))
    }
}

impl TryFrom<url::Url> for BigtableParameters {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(mut url: url::Url) -> Result<Self, Self::Error> {
        // parse the instance name from the hostname.
        let instance_name = url
            .host_str()
            .ok_or_else(|| Error::StorageError("instance name missing".into()))?
            .to_string();

        // … but add it to the query string now, so we just need to parse that.
        url.query_pairs_mut()
            .append_pair("instance_name", &instance_name);

        let params: BigtableParameters = serde_qs::from_str(url.query().unwrap_or_default())
            .map_err(|e| Error::InvalidRequest(format!("failed to parse parameters: {}", e)))?;

        Ok(params)
    }
}

fn default_app_profile_id() -> String {
    "default".to_owned()
}

fn default_channel_size() -> usize {
    4
}

fn default_timeout() -> Option<std::time::Duration> {
    Some(std::time::Duration::from_secs(4))
}
