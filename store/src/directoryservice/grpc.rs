use std::collections::HashSet;

use super::{DirectoryPutter, DirectoryService};
use crate::proto::{self, get_directory_request::ByWhat};
use crate::{B3Digest, Error};
use tokio::net::UnixStream;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::UnboundedReceiverStream;
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
    /// construct a [GRPCDirectoryService] from a [proto::directory_service_client::DirectoryServiceClient].
    /// panics if called outside the context of a tokio runtime.
    pub fn from_client(
        grpc_client: proto::directory_service_client::DirectoryServiceClient<Channel>,
    ) -> Self {
        Self {
            tokio_handle: tokio::runtime::Handle::current(),
            grpc_client,
        }
    }
}

impl DirectoryService for GRPCDirectoryService {
    /// Constructs a [GRPCDirectoryService] from the passed [url::Url]:
    /// - scheme has to match `grpc+*://`.
    ///   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
    /// - In the case of unix sockets, there must be a path, but may not be a host.
    /// - In the case of non-unix sockets, there must be a host, but no path.
    fn from_url(url: &url::Url) -> Result<Self, crate::Error> {
        // Start checking for the scheme to start with grpc+.
        match url.scheme().strip_prefix("grpc+") {
            None => Err(crate::Error::StorageError("invalid scheme".to_string())),
            Some(rest) => {
                if rest == "unix" {
                    if url.host_str().is_some() {
                        return Err(crate::Error::StorageError(
                            "host may not be set".to_string(),
                        ));
                    }
                    let path = url.path().to_string();
                    let channel = tonic::transport::Endpoint::try_from("http://[::]:50051") // doesn't matter
                        .unwrap()
                        .connect_with_connector_lazy(tower::service_fn(
                            move |_: tonic::transport::Uri| UnixStream::connect(path.clone()),
                        ));
                    let grpc_client =
                        proto::directory_service_client::DirectoryServiceClient::new(channel);
                    Ok(Self::from_client(grpc_client))
                } else {
                    // ensure path is empty, not supported with gRPC.
                    if !url.path().is_empty() {
                        return Err(crate::Error::StorageError(
                            "path may not be set".to_string(),
                        ));
                    }

                    // clone the uri, and drop the grpc+ from the scheme.
                    // Recreate a new uri with the `grpc+` prefix dropped from the scheme.
                    // We can't use `url.set_scheme(rest)`, as it disallows
                    // setting something http(s) that previously wasn't.
                    let url = {
                        let url_str = url.to_string();
                        let s_stripped = url_str.strip_prefix("grpc+").unwrap();
                        url::Url::parse(s_stripped).unwrap()
                    };
                    let channel = tonic::transport::Endpoint::try_from(url.to_string())
                        .unwrap()
                        .connect_lazy();

                    let grpc_client =
                        proto::directory_service_client::DirectoryServiceClient::new(channel);
                    Ok(Self::from_client(grpc_client))
                }
            }
        }
    }
    fn get(&self, digest: &B3Digest) -> Result<Option<crate::proto::Directory>, crate::Error> {
        // Get a new handle to the gRPC client, and copy the digest.
        let mut grpc_client = self.grpc_client.clone();
        let digest_cpy = digest.clone();
        let task = self.tokio_handle.spawn(async move {
            let mut s = grpc_client
                .get(proto::GetDirectoryRequest {
                    recursive: false,
                    by_what: Some(ByWhat::Digest(digest_cpy.into())),
                })
                .await?
                .into_inner();

            // Retrieve the first message only, then close the stream (we set recursive to false)
            s.message().await
        });

        let digest = digest.clone();
        match self.tokio_handle.block_on(task)? {
            Ok(Some(directory)) => {
                // Validate the retrieved Directory indeed has the
                // digest we expect it to have, to detect corruptions.
                let actual_digest = directory.digest();
                if actual_digest != digest {
                    Err(crate::Error::StorageError(format!(
                        "requested directory with digest {}, but got {}",
                        digest, actual_digest
                    )))
                } else if let Err(e) = directory.validate() {
                    // Validate the Directory itself is valid.
                    warn!("directory failed validation: {}", e.to_string());
                    Err(crate::Error::StorageError(format!(
                        "directory {} failed validation: {}",
                        digest, e,
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

    fn put(&self, directory: crate::proto::Directory) -> Result<B3Digest, crate::Error> {
        let mut grpc_client = self.grpc_client.clone();

        let task = self
            .tokio_handle
            .spawn(async move { grpc_client.put(tokio_stream::iter(vec![directory])).await });

        match self.tokio_handle.block_on(task)? {
            Ok(put_directory_resp) => Ok(put_directory_resp
                .into_inner()
                .root_digest
                .try_into()
                .map_err(|_| {
                    Error::StorageError("invalid root digest length in response".to_string())
                })?),
            Err(e) => Err(crate::Error::StorageError(e.to_string())),
        }
    }

    #[instrument(skip_all, fields(directory.digest = %root_directory_digest))]
    fn get_recursive(
        &self,
        root_directory_digest: &B3Digest,
    ) -> Box<dyn Iterator<Item = Result<proto::Directory, Error>> + Send> {
        let mut grpc_client = self.grpc_client.clone();

        // clone so we can move it
        let root_directory_digest_cpy = root_directory_digest.clone();

        let task: JoinHandle<Result<Streaming<proto::Directory>, Status>> =
            self.tokio_handle.spawn(async move {
                let s = grpc_client
                    .get(proto::GetDirectoryRequest {
                        recursive: true,
                        by_what: Some(ByWhat::Digest(root_directory_digest_cpy.into())),
                    })
                    .await?
                    .into_inner();

                Ok(s)
            });

        let stream = self.tokio_handle.block_on(task).unwrap().unwrap();

        Box::new(StreamIterator::new(
            self.tokio_handle.clone(),
            root_directory_digest.clone(),
            stream,
        ))
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<(dyn DirectoryPutter + 'static)>
    where
        Self: Clone,
    {
        let mut grpc_client = self.grpc_client.clone();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let task: JoinHandle<Result<proto::PutDirectoryResponse, Status>> =
            self.tokio_handle.spawn(async move {
                let s = grpc_client
                    .put(UnboundedReceiverStream::new(rx))
                    .await?
                    .into_inner();

                Ok(s)
            });

        Box::new(GRPCPutter::new(self.tokio_handle.clone(), tx, task))
    }
}

pub struct StreamIterator {
    /// A handle into the active tokio runtime. Necessary to run futures to completion.
    tokio_handle: tokio::runtime::Handle,
    // A stream of [proto::Directory]
    stream: Streaming<proto::Directory>,
    // The Directory digests we received so far
    received_directory_digests: HashSet<B3Digest>,
    // The Directory digests we're still expecting to get sent.
    expected_directory_digests: HashSet<B3Digest>,
}

impl StreamIterator {
    pub fn new(
        tokio_handle: tokio::runtime::Handle,
        root_digest: B3Digest,
        stream: Streaming<proto::Directory>,
    ) -> Self {
        Self {
            tokio_handle,
            stream,
            received_directory_digests: HashSet::new(),
            expected_directory_digests: HashSet::from([root_digest]),
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
                            directory.digest(),
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
                            directory_digest
                        ))));
                    }
                    self.received_directory_digests.insert(directory_digest);

                    // register all children in expected_directory_digests.
                    for child_directory in &directory.directories {
                        // We ran validate() above, so we know these digests must be correct.
                        let child_directory_digest =
                            child_directory.digest.clone().try_into().unwrap();

                        self.expected_directory_digests
                            .insert(child_directory_digest);
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

/// Allows uploading multiple Directory messages in the same gRPC stream.
pub struct GRPCPutter {
    /// A handle into the active tokio runtime. Necessary to spawn tasks.
    tokio_handle: tokio::runtime::Handle,

    /// Data about the current request - a handle to the task, and the tx part
    /// of the channel.
    /// The tx part of the pipe is used to send [proto::Directory] to the ongoing request.
    /// The task will yield a [proto::PutDirectoryResponse] once the stream is closed.
    #[allow(clippy::type_complexity)] // lol
    rq: Option<(
        JoinHandle<Result<proto::PutDirectoryResponse, Status>>,
        UnboundedSender<proto::Directory>,
    )>,
}

impl GRPCPutter {
    pub fn new(
        tokio_handle: tokio::runtime::Handle,
        directory_sender: UnboundedSender<proto::Directory>,
        task: JoinHandle<Result<proto::PutDirectoryResponse, Status>>,
    ) -> Self {
        Self {
            tokio_handle,
            rq: Some((task, directory_sender)),
        }
    }
}

impl DirectoryPutter for GRPCPutter {
    fn put(&mut self, directory: proto::Directory) -> Result<(), crate::Error> {
        match self.rq {
            // If we're not already closed, send the directory to directory_sender.
            Some((_, ref directory_sender)) => {
                if directory_sender.send(directory).is_err() {
                    // If the channel has been prematurely closed, invoke close (so we can peek at the error code)
                    // That error code is much more helpful, because it
                    // contains the error message from the server.
                    self.close()?;
                }
                Ok(())
            }
            // If self.close() was already called, we can't put again.
            None => Err(Error::StorageError(
                "DirectoryPutter already closed".to_string(),
            )),
        }
    }

    /// Closes the stream for sending, and returns the value
    fn close(&mut self) -> Result<B3Digest, crate::Error> {
        // get self.rq, and replace it with None.
        // This ensures we can only close it once.
        match std::mem::take(&mut self.rq) {
            None => Err(Error::StorageError("already closed".to_string())),
            Some((task, directory_sender)) => {
                // close directory_sender, so blocking on task will finish.
                drop(directory_sender);

                let root_digest = self
                    .tokio_handle
                    .block_on(task)?
                    .map_err(|e| Error::StorageError(e.to_string()))?
                    .root_digest;

                root_digest.try_into().map_err(|_| {
                    Error::StorageError("invalid root digest length in response".to_string())
                })
            }
        }
    }

    // allows checking if the tx part of the channel is closed.
    fn is_closed(&self) -> bool {
        match self.rq {
            None => true,
            Some((_, ref directory_sender)) => directory_sender.is_closed(),
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
        tests::{
            fixtures::{DIRECTORY_A, DIRECTORY_B},
            utils::gen_directory_service,
        },
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

        // wait for the socket to be created
        {
            let mut socket_created = false;
            for _try in 1..20 {
                if socket_path.exists() {
                    socket_created = true;
                    break;
                }
                std::thread::sleep(time::Duration::from_millis(20))
            }

            assert!(
                socket_created,
                "expected socket path to eventually get created, but never happened"
            );
        }

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
            let directory_service = super::GRPCDirectoryService::from_client(grpc_client);

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

            // And retrieve it, compare for equality.
            assert_eq!(
                DIRECTORY_A.clone(),
                directory_service
                    .get(&DIRECTORY_A.digest())
                    .expect("must succeed")
                    .expect("must be some")
            );

            // Putting DIRECTORY_B alone should fail, because it refers to DIRECTORY_A.
            directory_service
                .put(DIRECTORY_B.clone())
                .expect_err("must fail");

            // Putting DIRECTORY_B in a put_multiple will succeed, but the close
            // will always fail.
            {
                let mut handle = directory_service.put_multiple_start();
                handle.put(DIRECTORY_B.clone()).expect("must succeed");
                handle.close().expect_err("must fail");
            }

            // Uploading A and then B should succeed, and closing should return the digest of B.
            let mut handle = directory_service.put_multiple_start();
            handle.put(DIRECTORY_A.clone()).expect("must succeed");
            handle.put(DIRECTORY_B.clone()).expect("must succeed");
            let digest = handle.close().expect("must succeed");
            assert_eq!(DIRECTORY_B.digest(), digest);

            // Now try to retrieve the closure of DIRECTORY_B, which should return B and then A.
            let mut directories_it = directory_service.get_recursive(&DIRECTORY_B.digest());
            assert_eq!(
                DIRECTORY_B.clone(),
                directories_it
                    .next()
                    .expect("must be some")
                    .expect("must succeed")
            );
            assert_eq!(
                DIRECTORY_A.clone(),
                directories_it
                    .next()
                    .expect("must be some")
                    .expect("must succeed")
            );

            // Uploading B and then A should fail, because B refers to A, which
            // hasn't been uploaded yet.
            // However, the client can burst, so we might not have received the
            // error back from the server.
            {
                let mut handle = directory_service.put_multiple_start();
                // sending out B will always be fine
                handle.put(DIRECTORY_B.clone()).expect("must succeed");

                // whether we will be able to put A as well depends on whether we
                // already received the error about B.
                if handle.put(DIRECTORY_A.clone()).is_ok() {
                    // If we didn't, and this was Ok(_), …
                    // a subsequent close MUST fail (because it waits for the
                    // server)
                    handle.close().expect_err("must fail");
                }
            }

            // Now we do the same test as before, send B, then A, but wait
            // sufficiently enough for the server to have s
            // to close us the stream,
            // and then assert that uploading anything else via the handle will fail.
            {
                let mut handle = directory_service.put_multiple_start();
                handle.put(DIRECTORY_B.clone()).expect("must succeed");

                let mut is_closed = false;
                for _try in 1..1000 {
                    if handle.is_closed() {
                        is_closed = true;
                        break;
                    }
                    std::thread::sleep(time::Duration::from_millis(10))
                }

                assert!(
                    is_closed,
                    "expected channel to eventually close, but never happened"
                );

                handle.put(DIRECTORY_A.clone()).expect_err("must fail");
            }
        });

        tester_runtime.block_on(task)?;

        Ok(())
    }
}
