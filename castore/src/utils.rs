//! A crate containing constructors to provide instances of a BlobService and
//! DirectoryService. Only used for testing purposes, but across crates.
//! Should be removed once we have a better concept of a "Service registry".

use std::sync::Arc;
use tonic::transport::{Channel, Endpoint, Server, Uri};

use crate::{
    blobservice::{BlobService, MemoryBlobService},
    directoryservice::{DirectoryService, MemoryDirectoryService},
    proto::{
        blob_service_client::BlobServiceClient, blob_service_server::BlobServiceServer,
        directory_service_client::DirectoryServiceClient,
        directory_service_server::DirectoryServiceServer, GRPCBlobServiceWrapper,
        GRPCDirectoryServiceWrapper,
    },
};

pub fn gen_blob_service() -> Arc<dyn BlobService> {
    Arc::new(MemoryBlobService::default())
}

pub fn gen_directory_service() -> Arc<dyn DirectoryService> {
    Arc::new(MemoryDirectoryService::default())
}

/// This will spawn the a gRPC server with a DirectoryService client, connect a
/// gRPC DirectoryService client and return it.
#[allow(dead_code)]
pub(crate) async fn gen_directorysvc_grpc_client() -> DirectoryServiceClient<Channel> {
    let (left, right) = tokio::io::duplex(64);

    // spin up a server, which will only connect once, to the left side.
    tokio::spawn(async {
        // spin up a new DirectoryService
        let mut server = Server::builder();
        let router = server.add_service(DirectoryServiceServer::new(
            GRPCDirectoryServiceWrapper::new(gen_directory_service()),
        ));

        router
            .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(left)))
            .await
    });

    // Create a client, connecting to the right side. The URI is unused.
    let mut maybe_right = Some(right);
    DirectoryServiceClient::new(
        Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let right = maybe_right.take().unwrap();
                async move { Ok::<_, std::io::Error>(right) }
            }))
            .await
            .unwrap(),
    )
}

/// This will spawn the a gRPC server with a BlobService client, connect a
/// gRPC BlobService client and return it.
#[allow(dead_code)]
pub(crate) async fn gen_blobsvc_grpc_client() -> BlobServiceClient<Channel> {
    let (left, right) = tokio::io::duplex(64);

    // spin up a server, which will only connect once, to the left side.
    tokio::spawn(async {
        // spin up a new DirectoryService
        let mut server = Server::builder();
        let router = server.add_service(BlobServiceServer::new(GRPCBlobServiceWrapper::from(
            gen_blob_service(),
        )));

        router
            .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(left)))
            .await
    });

    // Create a client, connecting to the right side. The URI is unused.
    let mut maybe_right = Some(right);
    BlobServiceClient::new(
        Endpoint::try_from("http://[::]:50051")
            .unwrap()
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let right = maybe_right.take().unwrap();
                async move { Ok::<_, std::io::Error>(right) }
            }))
            .await
            .unwrap(),
    )
}
