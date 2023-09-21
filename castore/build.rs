use std::io::Result;

fn main() -> Result<()> {
    #[allow(unused_mut)]
    let mut builder = tonic_build::configure();

    #[cfg(feature = "reflection")]
    {
        let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let descriptor_path = out_dir.join("tvix.castore.v1.bin");

        builder = builder.file_descriptor_set_path(descriptor_path);
    };

    // https://github.com/hyperium/tonic/issues/908
    let mut config = prost_build::Config::new();
    config.bytes(["."]);

    builder
        .build_server(true)
        .build_client(true)
        .compile_with_config(
            config,
            &[
                "tvix/castore/protos/castore.proto",
                "tvix/castore/protos/rpc_blobstore.proto",
                "tvix/castore/protos/rpc_directory.proto",
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