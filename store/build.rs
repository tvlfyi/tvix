use std::io::Result;

fn main() -> Result<()> {
    prost_build::compile_protos(
        &[
            "tvix/store/protos/castore.proto",
            "tvix/store/protos/pathinfo.proto",
            "tvix/store/protos/rpc_blobstore.proto",
            "tvix/store/protos/rpc_pathinfo.proto",
        ],
        // If we are in running `cargo build` manually, using `../..` works fine,
        // but in case we run inside a nix build, we need to instead point PROTO_ROOT
        // to a sparseTree containing that structure.
        &[match std::env::var_os(&"PROTO_ROOT") {
            Some(proto_root) => proto_root.to_str().unwrap().to_owned(),
            None => "../..".to_string(),
        }],
    )?;
    Ok(())
}
