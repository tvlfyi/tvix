mod bundle;
mod spec;

pub(crate) use bundle::get_host_output_paths;
pub(crate) use bundle::make_bundle;
pub(crate) use spec::make_spec;

/// For a given scratch path, return the scratch_name that's allocated.
// We currently use use lower hex encoding of the b3 digest of the scratch
// path, so we don't need to globally allocate and pass down some uuids.
pub(crate) fn scratch_name(scratch_path: &str) -> String {
    data_encoding::BASE32.encode(blake3::hash(scratch_path.as_bytes()).as_bytes())
}
