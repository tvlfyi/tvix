//! Module to create a OCI runtime spec for a given [BuildRequest].
use crate::buildservice::{BuildConstraints, BuildRequest};
use oci_spec::{
    runtime::{Capability, LinuxNamespace, LinuxNamespaceBuilder, LinuxNamespaceType},
    OciSpecError,
};
use std::{collections::HashSet, path::Path};

use super::scratch_name;

/// For a given [BuildRequest], return an OCI runtime spec.
///
/// While there's no IO occuring in this function, the generated spec contains
/// path references relative to the "bundle location".
/// Due to overlayfs requiring its layers to be absolute paths, we also need a
/// [bundle_dir] parameter, pointing to the location of the bundle dir itself.
///
/// The paths used in the spec are the following (relative to a "bundle root"):
///
/// - `inputs`, a directory where the castore nodes specified the build request
///   inputs are supposed to be populated.
/// - `outputs`, a directory where all writes to the store_dir during the build
///   are directed to.
/// - `root`, a minimal skeleton of files that'll be present at /.
/// - `scratch`, a directory containing other directories which will be
///   bind-mounted read-write into the container and used as scratch space
///   during the build.
///   No assumptions should be made about what's inside this directory.
///
/// Generating these paths, and populating contents, like a skeleton root
/// is up to another function, this function doesn't do filesystem IO.
pub(crate) fn make_spec(
    request: &BuildRequest,
    rootless: bool,
    sandbox_shell: &str,
) -> Result<oci_spec::runtime::Spec, oci_spec::OciSpecError> {
    let allow_network = request
        .constraints
        .contains(&BuildConstraints::NetworkAccess);

    // Assemble ro_host_mounts. Start with constraints.available_ro_paths.
    let mut ro_host_mounts: Vec<_> = request
        .constraints
        .iter()
        .filter_map(|constraint| match constraint {
            BuildConstraints::AvailableReadOnlyPath(path) => Some((path.as_path(), path.as_path())),
            _ => None,
        })
        .collect();

    // If provide_bin_sh is set, mount sandbox_shell to /bin/sh
    if request
        .constraints
        .contains(&BuildConstraints::ProvideBinSh)
    {
        ro_host_mounts.push((Path::new(sandbox_shell), Path::new("/bin/sh")))
    }

    oci_spec::runtime::SpecBuilder::default()
        .process(configure_process(
            &request.command_args,
            &request.working_dir,
            request
                .environment_vars
                .iter()
                .map(|e| {
                    (
                        e.key.as_str(),
                        // TODO: decide what to do with non-bytes env values
                        String::from_utf8(e.value.to_vec()).expect("invalid string in env"),
                    )
                })
                .collect::<Vec<_>>(),
            rootless,
        )?)
        .linux(configure_linux(allow_network, rootless)?)
        .root(
            oci_spec::runtime::RootBuilder::default()
                .path("root")
                .readonly(true)
                .build()?,
        )
        .hostname("localhost")
        .mounts(configure_mounts(
            rootless,
            allow_network,
            request.scratch_paths.iter().map(|e| e.as_path()),
            request.inputs.iter(),
            &request.inputs_dir,
            ro_host_mounts,
        )?)
        .build()
}

/// Return the Process part of the OCI Runtime spec.
/// This configures the command, it's working dir, env and terminal setup.
/// It also takes care of setting rlimits and capabilities.
/// Capabilities are a bit more complicated in case rootless building is requested.
fn configure_process<'a>(
    command_args: &[String],
    cwd: &Path,
    env: impl IntoIterator<Item = (&'a str, String)>,
    rootless: bool,
) -> Result<oci_spec::runtime::Process, oci_spec::OciSpecError> {
    let spec_builder = oci_spec::runtime::ProcessBuilder::default()
        .args(command_args)
        .env(
            env.into_iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>(),
        )
        .terminal(true)
        .user(
            oci_spec::runtime::UserBuilder::default()
                .uid(1000u32)
                .gid(100u32)
                .build()?,
        )
        .cwd(Path::new("/").join(cwd)) // relative to the bundle root, but at least runc wants it to also be absolute.
        .capabilities({
            let caps: HashSet<Capability> = if !rootless {
                HashSet::from([Capability::AuditWrite, Capability::Kill])
            } else {
                HashSet::from([
                    Capability::AuditWrite,
                    Capability::Chown,
                    Capability::DacOverride,
                    Capability::Fowner,
                    Capability::Fsetid,
                    Capability::Kill,
                    Capability::Mknod,
                    Capability::NetBindService,
                    Capability::NetRaw,
                    Capability::Setfcap,
                    Capability::Setgid,
                    Capability::Setpcap,
                    Capability::Setuid,
                    Capability::SysChroot,
                ])
            };

            oci_spec::runtime::LinuxCapabilitiesBuilder::default()
                .bounding(caps.clone())
                .effective(caps.clone())
                .inheritable(caps.clone())
                .permitted(caps.clone())
                .ambient(caps)
                .build()?
        })
        .rlimits([oci_spec::runtime::PosixRlimitBuilder::default()
            .typ(oci_spec::runtime::PosixRlimitType::RlimitNofile)
            .hard(1024_u64)
            .soft(1024_u64)
            .build()?])
        .no_new_privileges(true);

    spec_builder.build()
}

/// Return the Linux part of the OCI Runtime spec.
/// This configures various namespaces, masked and read-only paths.
fn configure_linux(
    allow_network: bool,
    rootless: bool,
) -> Result<oci_spec::runtime::Linux, OciSpecError> {
    let mut linux = oci_spec::runtime::Linux::default();

    // explicitly set namespaces, depending on allow_network.
    linux.set_namespaces(Some({
        let mut namespace_types = vec![
            LinuxNamespaceType::Pid,
            LinuxNamespaceType::Ipc,
            LinuxNamespaceType::Uts,
            LinuxNamespaceType::Mount,
            LinuxNamespaceType::Cgroup,
        ];
        if !allow_network {
            namespace_types.push(LinuxNamespaceType::Network)
        }
        if rootless {
            namespace_types.push(LinuxNamespaceType::User)
        }

        namespace_types
            .into_iter()
            .map(|e| LinuxNamespaceBuilder::default().typ(e).build())
            .collect::<Result<Vec<LinuxNamespace>, _>>()?
    }));

    linux.set_masked_paths(Some(
        [
            "/proc/kcore",
            "/proc/latency_stats",
            "/proc/timer_list",
            "/proc/timer_stats",
            "/proc/sched_debug",
            "/sys/firmware",
        ]
        .into_iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>(),
    ));

    linux.set_readonly_paths(Some(
        [
            "/proc/asound",
            "/proc/bus",
            "/proc/fs",
            "/proc/irq",
            "/proc/sys",
            "/proc/sysrq-trigger",
        ]
        .into_iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>(),
    ));

    Ok(linux)
}

/// Return the Mounts part of the OCI Runtime spec.
/// It first sets up the standard mounts, then scratch paths, bind mounts for
/// all inputs, and finally read-only paths from the hosts.
fn configure_mounts<'a>(
    rootless: bool,
    allow_network: bool,
    scratch_paths: impl IntoIterator<Item = &'a Path>,
    inputs: impl Iterator<Item = (&'a tvix_castore::PathComponent, &'a tvix_castore::Node)>,

    inputs_dir: &Path,
    ro_host_mounts: impl IntoIterator<Item = (&'a Path, &'a Path)>,
) -> Result<Vec<oci_spec::runtime::Mount>, oci_spec::OciSpecError> {
    let mut mounts: Vec<_> = if rootless {
        oci_spec::runtime::get_rootless_mounts()
    } else {
        oci_spec::runtime::get_default_mounts()
    };

    mounts.push(configure_mount(
        Path::new("tmpfs"),
        Path::new("/tmp"),
        "tmpfs",
        &["nosuid", "noatime", "mode=700"],
    )?);

    // For each scratch path, create a bind mount entry.
    let scratch_root = Path::new("scratch"); // relative path
    for scratch_path in scratch_paths.into_iter() {
        let src = scratch_root.join(scratch_name(scratch_path));
        mounts.push(configure_mount(
            &src,
            &Path::new("/").join(scratch_path),
            "none",
            &["rbind", "rw"],
        )?);
    }

    // For each input, create a bind mount from inputs/$name into $inputs_dir/$name.
    for (input_name, _input) in inputs {
        let input_name = std::str::from_utf8(input_name.as_ref()).expect("invalid input name");
        mounts.push(configure_mount(
            &Path::new("inputs").join(input_name),
            &Path::new("/").join(inputs_dir).join(input_name),
            "none",
            &[
                "rbind", "ro",
                // "nosuid" is required, otherwise mounting will just fail with
                // a generic permission error.
                // See https://github.com/wllenyj/containerd/commit/42a386c8164bef16d59590c61ab00806f854d8fd
                "nosuid", "nodev",
            ],
        )?);
    }

    // Process ro_host_mounts
    for (src, dst) in ro_host_mounts.into_iter() {
        mounts.push(configure_mount(src, dst, "none", &["rbind", "ro"])?);
    }

    // In case network is enabled, also mount in /etc/{resolv.conf,services,hosts}
    if allow_network {
        for p in [
            Path::new("/etc/resolv.conf"),
            Path::new("/etc/services"),
            Path::new("/etc/hosts"),
        ] {
            mounts.push(configure_mount(p, p, "none", &["rbind", "ro"])?);
        }
    }

    Ok(mounts)
}

/// Helper function to produce a mount.
fn configure_mount(
    source: &Path,
    destination: &Path,
    typ: &str,
    options: &[&str],
) -> Result<oci_spec::runtime::Mount, oci_spec::OciSpecError> {
    oci_spec::runtime::MountBuilder::default()
        .destination(destination)
        .typ(typ.to_string())
        .source(source)
        .options(options.iter().map(|e| e.to_string()).collect::<Vec<_>>())
        .build()
}
