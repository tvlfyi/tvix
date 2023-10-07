//! A crate containing constructors to provide instances of a BlobService and
//! DirectoryService.
//! Only used for testing purposes, but across crates.
//! Should be removed once we have a better concept of a "Service registry".

use core::time;
use std::{path::Path, sync::Arc, thread};

use tokio::net::{UnixListener, UnixStream};
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::{Channel, Endpoint, Server, Uri};

use crate::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    proto::{
        directory_service_client::DirectoryServiceClient,
        directory_service_server::DirectoryServiceServer, GRPCDirectoryServiceWrapper,
    },
};

pub fn gen_blob_service() -> Arc<dyn BlobService> {
    Arc::new(MemoryBlobService::default())
}

pub fn gen_directory_service() -> Arc<dyn DirectoryService> {
    Arc::new(MemoryDirectoryService::default())
}

/// This will spawn a separate thread, with its own tokio runtime, and start a gRPC server there.
/// Once it's listening, it'll start a gRPC client from the original thread, and return it.
/// FUTUREWORK: accept a closure to create the service, so we can test this with different ones.
#[allow(dead_code)]
pub(crate) async fn gen_directorysvc_grpc_client(tmpdir: &Path) -> DirectoryServiceClient<Channel> {
    let socket_path = tmpdir.join("socket");

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

    // wait for the socket to be created
    // TODO: pass around FDs instead?
    {
        let mut socket_created = false;
        for _try in 1..20 {
            if socket_path.exists() {
                socket_created = true;
                break;
            }
            tokio::time::sleep(time::Duration::from_millis(20)).await;
        }

        assert!(
            socket_created,
            "expected socket path to eventually get created, but never happened"
        );
    }

    // Create a channel, connecting to the uds at socket_path.
    // The URI is unused.
    let channel = Endpoint::try_from("http://[::]:50051")
        .unwrap()
        .connect_with_connector_lazy(tower::service_fn(move |_: Uri| {
            UnixStream::connect(socket_path.clone())
        }));

    let grpc_client = DirectoryServiceClient::new(channel);

    grpc_client
}
