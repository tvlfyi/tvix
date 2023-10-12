use std::collections::HashSet;
use std::pin::Pin;

use super::{DirectoryPutter, DirectoryService};
use crate::proto::{self, get_directory_request::ByWhat};
use crate::{B3Digest, Error};
use async_stream::try_stream;
use futures::Stream;
use tokio::spawn;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::async_trait;
use tonic::Code;
use tonic::{transport::Channel, Status};
use tracing::{instrument, warn};

/// Connects to a (remote) tvix-store DirectoryService over gRPC.
#[derive(Clone)]
pub struct GRPCDirectoryService {
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
        Self { grpc_client }
    }
}

#[async_trait]
impl DirectoryService for GRPCDirectoryService {
    /// Constructs a [GRPCDirectoryService] from the passed [url::Url]:
    /// - scheme has to match `grpc+*://`.
    ///   That's normally grpc+unix for unix sockets, and grpc+http(s) for the HTTP counterparts.
    /// - In the case of unix sockets, there must be a path, but may not be a host.
    /// - In the case of non-unix sockets, there must be a host, but no path.
    fn from_url(url: &url::Url) -> Result<Self, crate::Error> {
        let channel = crate::channel::from_url(url)?;
        Ok(Self::from_client(
            proto::directory_service_client::DirectoryServiceClient::new(channel),
        ))
    }

    async fn get(
        &self,
        digest: &B3Digest,
    ) -> Result<Option<crate::proto::Directory>, crate::Error> {
        // Get a new handle to the gRPC client, and copy the digest.
        let mut grpc_client = self.grpc_client.clone();
        let digest_cpy = digest.clone();
        let message = async move {
            let mut s = grpc_client
                .get(proto::GetDirectoryRequest {
                    recursive: false,
                    by_what: Some(ByWhat::Digest(digest_cpy.into())),
                })
                .await?
                .into_inner();

            // Retrieve the first message only, then close the stream (we set recursive to false)
            s.message().await
        };

        let digest = digest.clone();
        match message.await {
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

    async fn put(&self, directory: crate::proto::Directory) -> Result<B3Digest, crate::Error> {
        let resp = self
            .grpc_client
            .clone()
            .put(tokio_stream::once(directory))
            .await;

        match resp {
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
    ) -> Pin<Box<dyn Stream<Item = Result<proto::Directory, Error>> + Send>> {
        let mut grpc_client = self.grpc_client.clone();
        let root_directory_digest = root_directory_digest.clone();

        let stream = try_stream! {
            let mut stream = grpc_client
                .get(proto::GetDirectoryRequest {
                    recursive: true,
                    by_what: Some(ByWhat::Digest(root_directory_digest.clone().into())),
                })
                .await
                .map_err(|e| crate::Error::StorageError(e.to_string()))?
                .into_inner();

            // The Directory digests we received so far
            let mut received_directory_digests: HashSet<B3Digest> = HashSet::new();
            // The Directory digests we're still expecting to get sent.
            let mut expected_directory_digests: HashSet<B3Digest> = HashSet::from([root_directory_digest]);

            loop {
                match stream.message().await {
                    Ok(Some(directory)) => {
                        // validate the directory itself.
                        if let Err(e) = directory.validate() {
                            Err(crate::Error::StorageError(format!(
                                "directory {} failed validation: {}",
                                directory.digest(),
                                e,
                            )))?;
                        }
                        // validate we actually expected that directory, and move it from expected to received.
                        let directory_digest = directory.digest();
                        let was_expected = expected_directory_digests.remove(&directory_digest);
                        if !was_expected {
                            // FUTUREWORK: dumb clients might send the same stuff twice.
                            // as a fallback, we might want to tolerate receiving
                            // it if it's in received_directory_digests (as that
                            // means it once was in expected_directory_digests)
                            Err(crate::Error::StorageError(format!(
                                "received unexpected directory {}",
                                directory_digest
                            )))?;
                        }
                        received_directory_digests.insert(directory_digest);

                        // register all children in expected_directory_digests.
                        for child_directory in &directory.directories {
                            // We ran validate() above, so we know these digests must be correct.
                            let child_directory_digest =
                                child_directory.digest.clone().try_into().unwrap();

                            expected_directory_digests
                                .insert(child_directory_digest);
                        }

                        yield directory;
                    },
                    Ok(None) => {
                        // If we were still expecting something, that's an error.
                        if !expected_directory_digests.is_empty() {
                            Err(crate::Error::StorageError(format!(
                                "still expected {} directories, but got premature end of stream",
                                expected_directory_digests.len(),
                            )))?
                        } else {
                            return
                        }
                    },
                    Err(e) => {
                        Err(crate::Error::StorageError(e.to_string()))?;
                    },
                }
            }
        };

        Box::pin(stream)
    }

    #[instrument(skip_all)]
    fn put_multiple_start(&self) -> Box<(dyn DirectoryPutter + 'static)>
    where
        Self: Clone,
    {
        let mut grpc_client = self.grpc_client.clone();

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let task: JoinHandle<Result<proto::PutDirectoryResponse, Status>> = spawn(async move {
            let s = grpc_client
                .put(UnboundedReceiverStream::new(rx))
                .await?
                .into_inner();

            Ok(s)
        });

        Box::new(GRPCPutter::new(tx, task))
    }
}

/// Allows uploading multiple Directory messages in the same gRPC stream.
pub struct GRPCPutter {
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
        directory_sender: UnboundedSender<proto::Directory>,
        task: JoinHandle<Result<proto::PutDirectoryResponse, Status>>,
    ) -> Self {
        Self {
            rq: Some((task, directory_sender)),
        }
    }
}

#[async_trait]
impl DirectoryPutter for GRPCPutter {
    async fn put(&mut self, directory: proto::Directory) -> Result<(), crate::Error> {
        match self.rq {
            // If we're not already closed, send the directory to directory_sender.
            Some((_, ref directory_sender)) => {
                if directory_sender.send(directory).is_err() {
                    // If the channel has been prematurely closed, invoke close (so we can peek at the error code)
                    // That error code is much more helpful, because it
                    // contains the error message from the server.
                    self.close().await?;
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
    async fn close(&mut self) -> Result<B3Digest, crate::Error> {
        // get self.rq, and replace it with None.
        // This ensures we can only close it once.
        match std::mem::take(&mut self.rq) {
            None => Err(Error::StorageError("already closed".to_string())),
            Some((task, directory_sender)) => {
                // close directory_sender, so blocking on task will finish.
                drop(directory_sender);

                let root_digest = task
                    .await?
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
    use futures::StreamExt;
    use std::{sync::Arc, time::Duration};
    use tempfile::TempDir;
    use tokio::net::UnixListener;
    use tokio_retry::{strategy::ExponentialBackoff, Retry};
    use tokio_stream::wrappers::UnixListenerStream;

    use crate::{
        directoryservice::{DirectoryService, GRPCDirectoryService, MemoryDirectoryService},
        fixtures::{self, DIRECTORY_A, DIRECTORY_B},
        proto::GRPCDirectoryServiceWrapper,
        utils::gen_directorysvc_grpc_client,
    };

    #[tokio::test]
    async fn test() {
        // create the GrpcDirectoryService
        let directory_service =
            super::GRPCDirectoryService::from_client(gen_directorysvc_grpc_client().await);

        // try to get DIRECTORY_A should return Ok(None)
        assert_eq!(
            None,
            directory_service
                .get(&DIRECTORY_A.digest())
                .await
                .expect("must not fail")
        );

        // Now upload it
        assert_eq!(
            DIRECTORY_A.digest(),
            directory_service
                .put(DIRECTORY_A.clone())
                .await
                .expect("must succeed")
        );

        // And retrieve it, compare for equality.
        assert_eq!(
            DIRECTORY_A.clone(),
            directory_service
                .get(&DIRECTORY_A.digest())
                .await
                .expect("must succeed")
                .expect("must be some")
        );

        // Putting DIRECTORY_B alone should fail, because it refers to DIRECTORY_A.
        directory_service
            .put(DIRECTORY_B.clone())
            .await
            .expect_err("must fail");

        // Putting DIRECTORY_B in a put_multiple will succeed, but the close
        // will always fail.
        {
            let mut handle = directory_service.put_multiple_start();
            handle.put(DIRECTORY_B.clone()).await.expect("must succeed");
            handle.close().await.expect_err("must fail");
        }

        // Uploading A and then B should succeed, and closing should return the digest of B.
        let mut handle = directory_service.put_multiple_start();
        handle.put(DIRECTORY_A.clone()).await.expect("must succeed");
        handle.put(DIRECTORY_B.clone()).await.expect("must succeed");
        let digest = handle.close().await.expect("must succeed");
        assert_eq!(DIRECTORY_B.digest(), digest);

        // Now try to retrieve the closure of DIRECTORY_B, which should return B and then A.
        let mut directories_it = directory_service.get_recursive(&DIRECTORY_B.digest());
        assert_eq!(
            DIRECTORY_B.clone(),
            directories_it
                .next()
                .await
                .expect("must be some")
                .expect("must succeed")
        );
        assert_eq!(
            DIRECTORY_A.clone(),
            directories_it
                .next()
                .await
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
            handle.put(DIRECTORY_B.clone()).await.expect("must succeed");

            // whether we will be able to put A as well depends on whether we
            // already received the error about B.
            if handle.put(DIRECTORY_A.clone()).await.is_ok() {
                // If we didn't, and this was Ok(_), …
                // a subsequent close MUST fail (because it waits for the
                // server)
                handle.close().await.expect_err("must fail");
            }
        }

        // Now we do the same test as before, send B, then A, but wait
        // sufficiently enough for the server to have s
        // to close us the stream,
        // and then assert that uploading anything else via the handle will fail.
        {
            let mut handle = directory_service.put_multiple_start();
            handle.put(DIRECTORY_B.clone()).await.expect("must succeed");

            let mut is_closed = false;
            for _try in 1..1000 {
                if handle.is_closed() {
                    is_closed = true;
                    break;
                }
                tokio::time::sleep(time::Duration::from_millis(10)).await;
            }

            assert!(
                is_closed,
                "expected channel to eventually close, but never happened"
            );

            handle
                .put(DIRECTORY_A.clone())
                .await
                .expect_err("must fail");
        }
    }

    /// This ensures connecting via gRPC works as expected.
    #[tokio::test]
    async fn test_valid_unix_path_ping_pong() {
        let tmpdir = TempDir::new().unwrap();
        let socket_path = tmpdir.path().join("daemon");

        let path_clone = socket_path.clone();

        // Spin up a server
        tokio::spawn(async {
            let uds = UnixListener::bind(path_clone).unwrap();
            let uds_stream = UnixListenerStream::new(uds);

            // spin up a new server
            let mut server = tonic::transport::Server::builder();
            let router = server.add_service(
                crate::proto::directory_service_server::DirectoryServiceServer::new(
                    GRPCDirectoryServiceWrapper::from(
                        Arc::new(MemoryDirectoryService::default()) as Arc<dyn DirectoryService>
                    ),
                ),
            );
            router.serve_with_incoming(uds_stream).await
        });

        // wait for the socket to be created
        Retry::spawn(
            ExponentialBackoff::from_millis(20).max_delay(Duration::from_secs(10)),
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

        // prepare a client
        let grpc_client = {
            let url = url::Url::parse(&format!("grpc+unix://{}", socket_path.display()))
                .expect("must parse");
            GRPCDirectoryService::from_url(&url).expect("must succeed")
        };

        assert!(grpc_client
            .get(&fixtures::DIRECTORY_A.digest())
            .await
            .expect("must not fail")
            .is_none())
    }
}
