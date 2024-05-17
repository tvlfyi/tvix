use super::PathInfoService;
use crate::proto;
use crate::proto::PathInfo;
use async_stream::try_stream;
use bigtable_rs::{bigtable, google::bigtable::v2 as bigtable_v2};
use bytes::Bytes;
use data_encoding::HEXLOWER;
use futures::stream::BoxStream;
use nix_compat::nixbase32;
use prost::Message;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};
use tonic::async_trait;
use tracing::{instrument, trace};
use tvix_castore::Error;

/// There should not be more than 10 MiB in a single cell.
/// https://cloud.google.com/bigtable/docs/schema-design#cells
const CELL_SIZE_LIMIT: u64 = 10 * 1024 * 1024;

/// Provides a [DirectoryService] implementation using
/// [Bigtable](https://cloud.google.com/bigtable/docs/)
/// as an underlying K/V store.
///
/// # Data format
/// We use Bigtable as a plain K/V store.
/// The row key is the digest of the store path, in hexlower.
/// Inside the row, we currently have a single column/cell, again using the
/// hexlower store path digest.
/// Its value is the PathInfo message, serialized in canonical protobuf.
/// We currently only populate this column.
///
/// Listing is ranging over all rows, and calculate_nar is returning a
/// "unimplemented" error.
#[derive(Clone)]
pub struct BigtablePathInfoService {
    client: bigtable::BigTable,
    params: BigtableParameters,

    #[cfg(test)]
    #[allow(dead_code)]
    /// Holds the temporary directory containing the unix socket, and the
    /// spawned emulator process.
    emulator: std::sync::Arc<(tempfile::TempDir, async_process::Child)>,
}

/// Represents configuration of [BigtablePathInfoService].
/// This currently conflates both connect parameters and data model/client
/// behaviour parameters.
#[serde_as]
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
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

impl BigtableParameters {
    #[cfg(test)]
    pub fn default_for_tests() -> Self {
        Self {
            project_id: "project-1".into(),
            instance_name: "instance-1".into(),
            is_read_only: false,
            channel_size: default_channel_size(),
            timeout: default_timeout(),
            table_name: "table-1".into(),
            family_name: "cf1".into(),
            app_profile_id: default_app_profile_id(),
        }
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

impl BigtablePathInfoService {
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
            false,
            None,
        )?;

        Ok(Self {
            client: connection.client(),
            params,
            emulator: (tmpdir, emulator_process).into(),
        })
    }
}

/// Derives the row/column key for a given output path.
/// We use hexlower encoding, also because it can't be misinterpreted as RE2.
fn derive_pathinfo_key(digest: &[u8; 20]) -> String {
    HEXLOWER.encode(digest)
}

#[async_trait]
impl PathInfoService for BigtablePathInfoService {
    #[instrument(level = "trace", skip_all, fields(path_info.digest = nixbase32::encode(&digest)))]
    async fn get(&self, digest: [u8; 20]) -> Result<Option<PathInfo>, Error> {
        let mut client = self.client.clone();
        let path_info_key = derive_pathinfo_key(&digest);

        let request = bigtable_v2::ReadRowsRequest {
            app_profile_id: self.params.app_profile_id.to_string(),
            table_name: client.get_full_table_name(&self.params.table_name),
            rows_limit: 1,
            rows: Some(bigtable_v2::RowSet {
                row_keys: vec![path_info_key.clone().into()],
                row_ranges: vec![],
            }),
            // Filter selected family name, and column qualifier matching the digest.
            // The latter is to ensure we don't fail once we start adding more metadata.
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
                                        path_info_key.clone().into(),
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

        let (row_key, mut cells) = response.pop().unwrap();
        if row_key != path_info_key.as_bytes() {
            // This shouldn't happen, we requested this row key.
            return Err(Error::StorageError(
                "got wrong row key from bigtable".into(),
            ));
        }

        let cell = cells
            .pop()
            .ok_or_else(|| Error::StorageError("found no cells".into()))?;

        // Ensure there's only one cell (so no more left after the pop())
        // This shouldn't happen, We filter out other cells in our query.
        if !cells.is_empty() {
            return Err(Error::StorageError(
                "more than one cell returned from bigtable".into(),
            ));
        }

        // We also require the qualifier to be correct in the filter above,
        // so this shouldn't happen.
        if path_info_key.as_bytes() != cell.qualifier {
            return Err(Error::StorageError("unexpected cell qualifier".into()));
        }

        // Try to parse the value into a PathInfo message
        let path_info = proto::PathInfo::decode(Bytes::from(cell.value))
            .map_err(|e| Error::StorageError(format!("unable to decode pathinfo proto: {}", e)))?;

        let store_path = path_info
            .validate()
            .map_err(|e| Error::StorageError(format!("invalid PathInfo: {}", e)))?;

        if store_path.digest() != &digest {
            return Err(Error::StorageError("PathInfo has unexpected digest".into()));
        }

        Ok(Some(path_info))
    }

    #[instrument(level = "trace", skip_all, fields(path_info.root_node = ?path_info.node))]
    async fn put(&self, path_info: PathInfo) -> Result<PathInfo, Error> {
        let store_path = path_info
            .validate()
            .map_err(|e| Error::InvalidRequest(format!("pathinfo failed validation: {}", e)))?;

        let mut client = self.client.clone();
        let path_info_key = derive_pathinfo_key(store_path.digest());

        let data = path_info.encode_to_vec();
        if data.len() as u64 > CELL_SIZE_LIMIT {
            return Err(Error::StorageError(
                "PathInfo exceeds cell limit on Bigtable".into(),
            ));
        }

        let resp = client
            .check_and_mutate_row(bigtable_v2::CheckAndMutateRowRequest {
                table_name: client.get_full_table_name(&self.params.table_name),
                app_profile_id: self.params.app_profile_id.to_string(),
                row_key: path_info_key.clone().into(),
                predicate_filter: Some(bigtable_v2::RowFilter {
                    filter: Some(bigtable_v2::row_filter::Filter::ColumnQualifierRegexFilter(
                        path_info_key.clone().into(),
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
                                column_qualifier: path_info_key.clone().into(),
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

        Ok(path_info)
    }

    fn list(&self) -> BoxStream<'static, Result<PathInfo, Error>> {
        let mut client = self.client.clone();

        let request = bigtable_v2::ReadRowsRequest {
            app_profile_id: self.params.app_profile_id.to_string(),
            table_name: client.get_full_table_name(&self.params.table_name),
            filter: Some(bigtable_v2::RowFilter {
                filter: Some(bigtable_v2::row_filter::Filter::FamilyNameRegexFilter(
                    self.params.family_name.to_string(),
                )),
            }),
            ..Default::default()
        };

        let stream = try_stream! {
            // TODO: add pagination, we don't want to hold all of this in memory.
            let response = client
                .read_rows(request)
                .await
                .map_err(|e| Error::StorageError(format!("unable to read rows: {}", e)))?;

            for (row_key, mut cells) in response {
                let cell = cells
                    .pop()
                    .ok_or_else(|| Error::StorageError("found no cells".into()))?;

                // The cell must have the same qualifier as the row key
                if row_key != cell.qualifier {
                    Err(Error::StorageError("unexpected cell qualifier".into()))?;
                }

                // Ensure there's only one cell (so no more left after the pop())
                // This shouldn't happen, We filter out other cells in our query.
                if !cells.is_empty() {

                    Err(Error::StorageError(
                        "more than one cell returned from bigtable".into(),
                    ))?
                }

                // Try to parse the value into a PathInfo message.
                let path_info = proto::PathInfo::decode(Bytes::from(cell.value))
                    .map_err(|e| Error::StorageError(format!("unable to decode pathinfo proto: {}", e)))?;

                // Validate the containing PathInfo, ensure its StorePath digest
                // matches row key.
                let store_path = path_info
                    .validate()
                    .map_err(|e| Error::StorageError(format!("invalid PathInfo: {}", e)))?;

                if store_path.digest().as_slice() != row_key.as_slice() {
                    Err(Error::StorageError("PathInfo has unexpected digest".into()))?
                }


                yield path_info
            }
        };

        Box::pin(stream)
    }
}
