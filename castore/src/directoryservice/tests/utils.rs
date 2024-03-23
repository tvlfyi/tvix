use crate::directoryservice::{DirectoryService, GRPCDirectoryService};
use crate::proto::directory_service_client::DirectoryServiceClient;
use crate::proto::GRPCDirectoryServiceWrapper;
use crate::{
    directoryservice::MemoryDirectoryService,
    proto::directory_service_server::DirectoryServiceServer,
};
use tonic::transport::{Endpoint, Server, Uri};

/// Constructs and returns a gRPC DirectoryService.
/// The server part is a [MemoryDirectoryService], exposed via the
/// [GRPCDirectoryServiceWrapper], and connected through a DuplexStream.
pub async fn make_grpc_directory_service_client() -> Box<dyn DirectoryService> {
    let (left, right) = tokio::io::duplex(64);

    // spin up a server, which will only connect once, to the left side.
    tokio::spawn(async {
        let directory_service =
            Box::<MemoryDirectoryService>::default() as Box<dyn DirectoryService>;

        let mut server = Server::builder();
        let router = server.add_service(DirectoryServiceServer::new(
            GRPCDirectoryServiceWrapper::new(directory_service),
        ));

        router
            .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(left)))
            .await
    });

    // Create a client, connecting to the right side. The URI is unused.
    let mut maybe_right = Some(right);
    Box::new(GRPCDirectoryService::from_client(
        DirectoryServiceClient::new(
            Endpoint::try_from("http://[::]:50051")
                .unwrap()
                .connect_with_connector(tower::service_fn(move |_: Uri| {
                    let right = maybe_right.take().unwrap();
                    async move { Ok::<_, std::io::Error>(right) }
                }))
                .await
                .unwrap(),
        ),
    ))
}
