use std::collections::HashSet;

use super::DirectoryService;
use crate::proto::{self, get_directory_request::ByWhat};
use data_encoding::BASE64;
use tonic::{transport::Channel, Status};
use tonic::{Code, Streaming};
use tracing::{instrument, warn};

/// Connects to a (remote) tvix-store DirectoryService over gRPC.
#[derive(Clone)]
pub struct GRPCDirectoryService {
    /// A handle into the active tokio runtime. Necessary to spawn tasks.
    tokio_handle: tokio::runtime::Handle,

    /// The internal reference to a gRPC client.
    /// Cloning it is cheap, and it internally handles concurrent requests.
    grpc_client: proto::directory_service_client::DirectoryServiceClient<Channel>,
}

impl GRPCDirectoryService {
    /// Construct a new [GRPCDirectoryService], by passing a handle to the
    /// tokio runtime, and a gRPC client.
    pub fn new(
        tokio_handle: tokio::runtime::Handle,
        grpc_client: proto::directory_service_client::DirectoryServiceClient<Channel>,
    ) -> Self {
        Self {
            tokio_handle,
            grpc_client,
        }
    }
}

impl DirectoryService for GRPCDirectoryService {
    type DirectoriesIterator = StreamIterator;

    fn get(&self, digest: &[u8; 32]) -> Result<Option<crate::proto::Directory>, crate::Error> {
        // Get a new handle to the gRPC client, and copy the digest.
        let mut grpc_client = self.grpc_client.clone();
        let digest = digest.to_owned();

        let task = self.tokio_handle.spawn(async move {
            let mut s = grpc_client
                .get(proto::GetDirectoryRequest {
                    recursive: false,
                    by_what: Some(ByWhat::Digest(digest.to_vec())),
                })
                .await?
                .into_inner();

            // Retrieve the first message only, then close the stream (we set recursive to false)
            s.message().await
        });

        match self.tokio_handle.block_on(task)? {
            Ok(Some(directory)) => {
                // Validate the retrieved Directory indeed has the
                // digest we expect it to have, to detect corruptions.
                let actual_digest = directory.digest();
                if actual_digest != digest {
                    Err(crate::Error::StorageError(format!(
                        "requested directory with digest {}, but got {}",
                        BASE64.encode(&digest),
                        BASE64.encode(&actual_digest)
                    )))
                } else if let Err(e) = directory.validate() {
                    // Validate the Directory itself is valid.
                    warn!("directory failed validation: {}", e.to_string());
                    Err(crate::Error::StorageError(format!(
                        "directory {} failed validation: {}",
                        BASE64.encode(&digest),
                        e,
                    )))
                } else {
                    Ok(Some(directory))
                }
            }
            Ok(None) => Ok(None),
            Err(e) if e.code() == Code::NotFound => Ok(None),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    fn put(&self, directory: crate::proto::Directory) -> Result<[u8; 32], crate::Error> {
        let mut grpc_client = self.grpc_client.clone();

        // TODO: this currently doesn't work for directories referring to other
        // directories, as we're required to upload the whole closure all the
        // time.
        let task = self
            .tokio_handle
            .spawn(async move { grpc_client.put(tokio_stream::iter(vec![directory])).await });

        match self.tokio_handle.block_on(task)? {
            Ok(put_directory_resp) => Ok(put_directory_resp
                .into_inner()
                .root_digest
                .as_slice()
                .try_into()
                .unwrap()), // TODO: map error
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip_all, fields(directory.digest = BASE64.encode(root_directory_digest)))]
    fn get_recursive(&self, root_directory_digest: &[u8; 32]) -> Self::DirectoriesIterator {
        let mut grpc_client = self.grpc_client.clone();
        let root_directory_digest = root_directory_digest.to_owned();

        let task: tokio::task::JoinHandle<Result<Streaming<proto::Directory>, Status>> =
            self.tokio_handle.spawn(async move {
                let s = grpc_client
                    .get(proto::GetDirectoryRequest {
                        recursive: true,
                        by_what: Some(ByWhat::Digest(root_directory_digest.to_vec())),
                    })
                    .await?
                    .into_inner();

                Ok(s)
            });

        let stream = self.tokio_handle.block_on(task).unwrap().unwrap();

        StreamIterator::new(self.tokio_handle.clone(), &root_directory_digest, stream)
    }
}

pub struct StreamIterator {
    /// A handle into the active tokio runtime. Necessary to run futures to completion.
    tokio_handle: tokio::runtime::Handle,
    // A stream of [proto::Directory]
    stream: Streaming<proto::Directory>,
    // The Directory digests we received so far
    received_directory_digests: HashSet<[u8; 32]>,
    // The Directory digests we're still expecting to get sent.
    expected_directory_digests: HashSet<[u8; 32]>,
}

impl StreamIterator {
    pub fn new(
        tokio_handle: tokio::runtime::Handle,
        root_digest: &[u8; 32],
        stream: Streaming<proto::Directory>,
    ) -> Self {
        Self {
            tokio_handle,
            stream,
            received_directory_digests: HashSet::new(),
            expected_directory_digests: HashSet::from([*root_digest]),
        }
    }
}

impl Iterator for StreamIterator {
    type Item = Result<proto::Directory, crate::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.tokio_handle.block_on(self.stream.message()) {
            Ok(ok) => match ok {
                Some(directory) => {
                    // validate the directory itself.
                    if let Err(e) = directory.validate() {
                        return Some(Err(crate::Error::StorageError(format!(
                            "directory {} failed validation: {}",
                            BASE64.encode(&directory.digest()),
                            e,
                        ))));
                    }
                    // validate we actually expected that directory, and move it from expected to received.
                    let directory_digest = directory.digest();
                    let was_expected = self.expected_directory_digests.remove(&directory_digest);
                    if !was_expected {
                        // FUTUREWORK: dumb clients might send the same stuff twice.
                        // as a fallback, we might want to tolerate receiving
                        // it if it's in received_directory_digests (as that
                        // means it once was in expected_directory_digests)
                        return Some(Err(crate::Error::StorageError(format!(
                            "received unexpected directory {}",
                            BASE64.encode(&directory_digest)
                        ))));
                    }
                    self.received_directory_digests.insert(directory_digest);

                    // register all children in expected_directory_digests.
                    for child_directories in &directory.directories {
                        self.expected_directory_digests
                            .insert(child_directories.digest.clone().try_into().unwrap());
                    }

                    Some(Ok(directory))
                }
                None => {
                    // If we were still expecting something, that's an error.
                    if !self.expected_directory_digests.is_empty() {
                        Some(Err(crate::Error::StorageError(format!(
                            "still expected {} directories, but got premature end of stream",
                            self.expected_directory_digests.len(),
                        ))))
                    } else {
                        None
                    }
                }
            },
            Err(e) => Some(Err(crate::Error::StorageError(e.to_string()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::time;
    use std::thread;

    use tempfile::TempDir;
    use tokio::net::{UnixListener, UnixStream};
    use tokio_stream::wrappers::UnixListenerStream;
    use tonic::transport::{Endpoint, Server, Uri};

    use crate::{
        directoryservice::DirectoryService,
        proto,
        proto::{directory_service_server::DirectoryServiceServer, GRPCDirectoryServiceWrapper},
        tests::{fixtures::DIRECTORY_A, utils::gen_directory_service},
    };

    #[test]
    fn test() -> anyhow::Result<()> {
        let tmpdir = TempDir::new().unwrap();
        let socket_path = tmpdir.path().join("socket");

        // Spin up a server, in a thread far away, which spawns its own tokio runtime,
        // and blocks on the task.
        let socket_path_clone = socket_path.clone();
        thread::spawn(move || {
            // Create the runtime
            let rt = tokio::runtime::Runtime::new().unwrap();
            // Get a handle from this runtime
            let handle = rt.handle();

            let task = handle.spawn(async {
                let uds = UnixListener::bind(socket_path_clone).unwrap();
                let uds_stream = UnixListenerStream::new(uds);

                // spin up a new DirectoryService
                let mut server = Server::builder();
                let router = server.add_service(DirectoryServiceServer::new(
                    GRPCDirectoryServiceWrapper::from(gen_directory_service()),
                ));
                router.serve_with_incoming(uds_stream).await
            });

            handle.block_on(task)
        });

        // set up the local client runtime. This is similar to what the [tokio:test] macro desugars to.
        let tester_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // TODO: wait for the socket to be created
        std::thread::sleep(time::Duration::from_millis(200));

        let task = tester_runtime.spawn_blocking(move || {
            // Create a channel, connecting to the uds at socket_path.
            // The URI is unused.
            let channel = Endpoint::try_from("http://[::]:50051")
                .unwrap()
                .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
                    UnixStream::connect(socket_path.clone())
                }));

            let grpc_client = proto::directory_service_client::DirectoryServiceClient::new(channel);

            // create the GrpcDirectoryService, using the tester_runtime.
            let directory_service =
                super::GRPCDirectoryService::new(tokio::runtime::Handle::current(), grpc_client);

            // try to get DIRECTORY_A should return Ok(None)
            assert_eq!(
                None,
                directory_service
                    .get(&DIRECTORY_A.digest())
                    .expect("must not fail")
            );

            // Now upload it
            assert_eq!(
                DIRECTORY_A.digest(),
                directory_service
                    .put(DIRECTORY_A.clone())
                    .expect("must succeed")
            );

            // And retrieve it. We don't compare the two structs literally
            assert_eq!(
                DIRECTORY_A.clone(),
                directory_service
                    .get(&DIRECTORY_A.digest())
                    .expect("must succeed")
                    .expect("must be some")
            )
        });
        tester_runtime.block_on(task)?;

        Ok(())
    }
}
