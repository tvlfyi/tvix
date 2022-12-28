use crate::proto::blob_service_server::BlobServiceServer;
use crate::proto::directory_service_server::DirectoryServiceServer;
use crate::proto::path_info_service_server::PathInfoServiceServer;

use clap::Parser;
use tonic::{transport::Server, Result};

mod dummy_blob_service;
mod dummy_directory_service;
mod dummy_path_info_service;
mod nixbase32;
mod nixpath;
mod proto;

#[cfg(test)]
mod tests;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[clap(long, short = 'l')]
    listen_address: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let listen_address = cli
        .listen_address
        .unwrap_or("[::]:8000".to_string())
        .parse()
        .unwrap();

    let blob_service = dummy_blob_service::DummyBlobService {};
    let directory_service = dummy_directory_service::DummyDirectoryService {};
    let path_info_service = dummy_path_info_service::DummyPathInfoService {};

    println!("tvix-store listening on {}", listen_address);

    Server::builder()
        .add_service(BlobServiceServer::new(blob_service))
        .add_service(DirectoryServiceServer::new(directory_service))
        .add_service(PathInfoServiceServer::new(path_info_service))
        .serve(listen_address)
        .await?;

    Ok(())
}
