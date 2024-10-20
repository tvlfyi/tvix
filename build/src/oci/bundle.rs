//! Module to create an OCI runtime bundle for a given [BuildRequest].
use std::{
    fs,
    path::{Path, PathBuf},
};

use super::scratch_name;
use crate::buildservice::BuildRequest;
use anyhow::{bail, Context};
use tracing::{debug, instrument};

/// Produce an OCI bundle in a given path.
/// Check [make_spec] for a description about the paths produced.
#[instrument(err)]
pub(crate) fn make_bundle<'a>(
    request: &BuildRequest,
    runtime_spec: &oci_spec::runtime::Spec,
    path: &Path,
) -> anyhow::Result<()> {
    fs::create_dir_all(path).context("failed to create bundle path")?;

    let spec_json = serde_json::to_string(runtime_spec).context("failed to render spec to json")?;
    fs::write(path.join("config.json"), spec_json).context("failed to write config.json")?;

    fs::create_dir_all(path.join("inputs")).context("failed to create inputs dir")?;

    let root_path = path.join("root");

    fs::create_dir_all(&root_path).context("failed to create root path dir")?;
    fs::create_dir_all(root_path.join("etc")).context("failed to create root/etc dir")?;

    // TODO: populate /etc/{group,passwd}. It's a mess?

    let scratch_root = path.join("scratch");
    fs::create_dir_all(&scratch_root).context("failed to create scratch/ dir")?;

    // for each scratch path, calculate its name inside scratch, and ensure the
    // directory exists.
    for p in request.scratch_paths.iter() {
        let scratch_path = scratch_root.join(scratch_name(p));
        debug!(scratch_path=?scratch_path, path=?p, "about to create scratch dir");
        fs::create_dir_all(scratch_path).context("Unable to create scratch dir")?;
    }

    Ok(())
}

/// Determine the path of all outputs specified in a [BuildRequest]
/// as seen from the host, for post-build ingestion.
/// This lookup needs to take scratch paths into consideration, as the build
/// root is not writable on its own.
/// If a path can't be determined, an error is returned.
pub(crate) fn get_host_output_paths(
    request: &BuildRequest,
    bundle_path: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let scratch_root = bundle_path.join("scratch");

    let mut host_output_paths: Vec<PathBuf> = Vec::with_capacity(request.outputs.len());

    for output_path in request.outputs.iter() {
        // calculate the location of the path.
        if let Some((mp, relpath)) = find_path_in_scratchs(output_path, &request.scratch_paths) {
            host_output_paths.push(scratch_root.join(scratch_name(mp)).join(relpath));
        } else {
            bail!("unable to find path {output_path:?}");
        }
    }

    Ok(host_output_paths)
}

/// For a given list of mountpoints (sorted) and a search_path, find the
/// specific mountpoint parenting that search_path and return it, as well as the
/// relative path from there to the search_path.
/// mountpoints must be sorted, so we can iterate over the list from the back
/// and match on the prefix.
fn find_path_in_scratchs<'a, 'b, I>(
    search_path: &'a Path,
    mountpoints: I,
) -> Option<(&'b Path, &'a Path)>
where
    I: IntoIterator<Item = &'b PathBuf>,
    I::IntoIter: DoubleEndedIterator,
{
    mountpoints
        .into_iter()
        .rev()
        .find_map(|mp| Some((mp.as_path(), search_path.strip_prefix(mp).ok()?)))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use rstest::rstest;

    use crate::{buildservice::BuildRequest, oci::scratch_name};

    use super::{find_path_in_scratchs, get_host_output_paths};

    #[rstest]
    #[case::simple("nix/store/aaaa", &["nix/store".into()], Some(("nix/store", "aaaa")))]
    #[case::prefix_no_sep("nix/store/aaaa", &["nix/sto".into()], None)]
    #[case::not_found("nix/store/aaaa", &["build".into()], None)]
    fn test_test_find_path_in_scratchs(
        #[case] search_path: &str,
        #[case] mountpoints: &[String],
        #[case] expected: Option<(&str, &str)>,
    ) {
        let expected = expected.map(|e| (Path::new(e.0), Path::new(e.1)));
        assert_eq!(
            find_path_in_scratchs(
                Path::new(search_path),
                mountpoints
                    .iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>()
                    .as_slice()
            ),
            expected
        );
    }

    #[test]
    fn test_get_host_output_paths_simple() {
        let request = BuildRequest {
            outputs: vec!["nix/store/fhaj6gmwns62s6ypkcldbaj2ybvkhx3p-foo".into()],
            scratch_paths: vec!["build".into(), "nix/store".into()],
            ..Default::default()
        };

        let paths =
            get_host_output_paths(&request, Path::new("bundle-root")).expect("must succeed");

        let mut expected_path = PathBuf::new();
        expected_path.push("bundle-root");
        expected_path.push("scratch");
        expected_path.push(scratch_name(Path::new("nix/store")));
        expected_path.push("fhaj6gmwns62s6ypkcldbaj2ybvkhx3p-foo");

        assert_eq!(vec![expected_path], paths)
    }
}
