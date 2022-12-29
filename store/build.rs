use std::io::Result;

fn main() -> Result<()> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile(
            &[
                "tvix/store/protos/castore.proto",
                "tvix/store/protos/pathinfo.proto",
                "tvix/store/protos/rpc_blobstore.proto",
                "tvix/store/protos/rpc_directory.proto",
                "tvix/store/protos/rpc_pathinfo.proto",
            ],
            // If we are in running `cargo build` manually, using `../..` works fine,
            // but in case we run inside a nix build, we need to instead point PROTO_ROOT
            // to a sparseTree containing that structure.
            &[match std::env::var_os("PROTO_ROOT") {
                Some(proto_root) => proto_root.to_str().unwrap().to_owned(),
                None => "../..".to_string(),
            }],
        )?;
    Ok(())
}
