//! This module contains glue code translating from
//! [nix_compat::derivation::Derivation] to [tvix_build::proto::BuildRequest].

use std::collections::BTreeMap;

use bytes::Bytes;
use nix_compat::{derivation::Derivation, store_path::StorePathRef};
use tvix_build::proto::{
    build_request::{BuildConstraints, EnvVar},
    BuildRequest,
};
use tvix_castore::proto::{NamedNode, Node};

/// These are the environment variables that Nix sets in its sandbox for every
/// build.
const NIX_ENVIRONMENT_VARS: [(&str, &str); 12] = [
    ("HOME", "/homeless-shelter"),
    ("NIX_BUILD_CORES", "0"), // TODO: make this configurable?
    ("NIX_BUILD_TOP", "/"),
    ("NIX_LOG_FD", "2"),
    ("NIX_STORE", "/nix/store"),
    ("PATH", "/path-not-set"),
    ("PWD", "/build"),
    ("TEMP", "/build"),
    ("TEMPDIR", "/build"),
    ("TERM", "xterm-256color"),
    ("TMP", "/build"),
    ("TMPDIR", "/build"),
];

/// Takes a [Derivation] and turns it into a [BuildRequest].
/// It assumes the Derivation has been validated.
/// It needs two lookup functions:
/// - one translating input sources to a castore node
///   (`fn_input_sources_to_node`)
/// - one translating input derivations and (a subset of their) output names to
///   a castore node (`fn_input_drvs_to_node`).
#[allow(dead_code)]
fn derivation_to_build_request<FIS, FID>(
    derivation: &Derivation,
    fn_input_sources_to_node: FIS,
    fn_input_drvs_to_node: FID,
) -> BuildRequest
where
    FIS: Fn(StorePathRef) -> Node,
    FID: Fn(StorePathRef, &[&str]) -> Node,
{
    debug_assert!(derivation.validate(true).is_ok(), "drv must validate");

    // produce command_args, which is builder and arguments in a Vec.
    let mut command_args: Vec<String> = Vec::with_capacity(derivation.arguments.len() + 1);
    command_args.push(derivation.builder.clone());
    command_args.extend_from_slice(&derivation.arguments);

    // produce output_paths, which is the absolute path of each output (sorted)
    let mut output_paths: Vec<String> = derivation
        .outputs
        .values()
        .map(|e| e.path[1..].to_owned())
        .collect();

    // Sort the outputs. We can use sort_unstable, as these are unique strings.
    output_paths.sort_unstable();

    // Produce environment_vars. We use a BTreeMap while producing them, so the
    // resulting Vec is sorted by key.
    let mut environment_vars: BTreeMap<String, Bytes> = BTreeMap::new();

    // Start with some the ones that nix magically sets:
    environment_vars.extend(
        NIX_ENVIRONMENT_VARS
            .iter()
            .map(|(k, v)| (k.to_string(), Bytes::from_static(v.as_bytes()))),
    );

    // extend / overwrite with the keys set in the derivation environment itself.
    // TODO: check if this order is correct, and environment vars set in the
    // *Derivation actually* have priority.
    environment_vars.extend(
        derivation
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), Bytes::from(v.to_vec()))),
    );

    // Turn this into a sorted-by-key Vec<EnvVar>.
    let environment_vars = Vec::from_iter(
        environment_vars
            .into_iter()
            .map(|(k, v)| EnvVar { key: k, value: v }),
    );

    // Produce inputs. As we refer to the contents here, not just plain store path strings,
    // we need to perform lookups.
    // FUTUREWORK: should we also model input_derivations and input_sources with StorePath?
    let mut inputs: Vec<Node> = Vec::new();

    // since Derivation is validated, we know input sources can be parsed.
    for input_source in derivation.input_sources.iter() {
        let sp = StorePathRef::from_absolute_path(input_source.as_bytes())
            .expect("invalid input source path");
        let node = fn_input_sources_to_node(sp);
        inputs.push(node);
    }

    // since Derivation is validated, we know input derivations can be parsed.
    for (input_derivation, output_names) in derivation.input_derivations.iter() {
        let sp = StorePathRef::from_absolute_path(input_derivation.as_bytes())
            .expect("invalid input derivation path");
        let output_names: Vec<&str> = output_names.iter().map(|e| e.as_str()).collect();
        let node = fn_input_drvs_to_node(sp, output_names.as_slice());
        inputs.push(node);
    }

    // validate all nodes are actually Some.
    debug_assert!(
        inputs.iter().all(|input| input.node.is_some()),
        "input nodes must be some"
    );

    // sort inputs by their name
    inputs.sort_by(|a, b| {
        a.node
            .as_ref()
            .unwrap()
            .get_name()
            .cmp(b.node.as_ref().unwrap().get_name())
    });

    // Produce constraints.
    let constraints = Some(BuildConstraints {
        system: derivation.system.clone(),
        min_memory: 0,
        available_ro_paths: vec![],
        // in case this is a fixed-output derivation, allow network access.
        network_access: derivation.outputs.len() == 1
            && derivation
                .outputs
                .get("out")
                .expect("invalid derivation")
                .is_fixed(),
        provide_bin_sh: true,
    });

    BuildRequest {
        command_args,
        outputs: output_paths,
        environment_vars,
        inputs,
        inputs_dir: nix_compat::store_path::STORE_DIR.into(),
        constraints,
        working_dir: "build".into(),
        scratch_paths: vec!["build".into(), "nix/store".into()],
        // TODO: handle passAsFile, structuredAttrs.
        additional_files: vec![],
    }
}

#[cfg(test)]
mod test {
    use bytes::Bytes;
    use nix_compat::derivation::Derivation;
    use tvix_build::proto::{
        build_request::{BuildConstraints, EnvVar},
        BuildRequest,
    };
    use tvix_castore::{
        fixtures::DUMMY_DIGEST,
        proto::{DirectoryNode, Node},
    };

    use crate::tvix_build::NIX_ENVIRONMENT_VARS;

    use super::derivation_to_build_request;
    use lazy_static::lazy_static;

    lazy_static! {
        static ref INPUT_NODE_FOO: Node = Node {
            node: Some(tvix_castore::proto::node::Node::Directory(DirectoryNode {
                name: Bytes::from("mp57d33657rf34lzvlbpfa1gjfv5gmpg-bar"),
                digest: DUMMY_DIGEST.clone().into(),
                size: 42,
            })),
        };
    }

    #[test]
    fn test_derivation_to_build_request() {
        let aterm_bytes = include_bytes!("tests/ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv");

        let derivation = Derivation::from_aterm_bytes(aterm_bytes).expect("must parse");

        let build_request = derivation_to_build_request(
            &derivation,
            |_| unreachable!(),
            |input_drv, output_names| {
                // expected to be called with ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv only
                if input_drv.to_string() != "ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv" {
                    panic!("called with unexpected input_drv: {}", input_drv);
                }
                // expect to be called with ["out"]
                if output_names != ["out"] {
                    panic!("called with unexpected output_names: {:?}", output_names);
                }

                // all good, reply with INPUT_NODE_FOO
                INPUT_NODE_FOO.clone()
            },
        );

        let mut expected_environment_vars = vec![
            EnvVar {
                key: "bar".into(),
                value: "/nix/store/mp57d33657rf34lzvlbpfa1gjfv5gmpg-bar".into(),
            },
            EnvVar {
                key: "builder".into(),
                value: ":".into(),
            },
            EnvVar {
                key: "name".into(),
                value: "foo".into(),
            },
            EnvVar {
                key: "out".into(),
                value: "/nix/store/fhaj6gmwns62s6ypkcldbaj2ybvkhx3p-foo".into(),
            },
            EnvVar {
                key: "system".into(),
                value: ":".into(),
            },
        ];

        expected_environment_vars.extend(NIX_ENVIRONMENT_VARS.iter().map(|(k, v)| EnvVar {
            key: k.to_string(),
            value: Bytes::from_static(v.as_bytes()),
        }));

        expected_environment_vars.sort_unstable_by_key(|e| e.key.to_owned());

        assert_eq!(
            BuildRequest {
                command_args: vec![":".into()],
                outputs: vec!["nix/store/fhaj6gmwns62s6ypkcldbaj2ybvkhx3p-foo".into()],
                environment_vars: expected_environment_vars,
                inputs: vec![INPUT_NODE_FOO.clone()],
                inputs_dir: nix_compat::store_path::STORE_DIR.into(),
                constraints: Some(BuildConstraints {
                    system: derivation.system.clone(),
                    min_memory: 0,
                    network_access: false,
                    available_ro_paths: vec![],
                    provide_bin_sh: true,
                }),
                additional_files: vec![],
                working_dir: "build".into(),
                scratch_paths: vec!["build".into(), "nix/store".into()],
            },
            build_request
        );
    }

    #[test]
    fn test_fod_to_build_request() {
        let aterm_bytes = include_bytes!("tests/0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv");

        let derivation = Derivation::from_aterm_bytes(aterm_bytes).expect("must parse");

        let build_request =
            derivation_to_build_request(&derivation, |_| unreachable!(), |_, _| unreachable!());

        let mut expected_environment_vars = vec![
            EnvVar {
                key: "builder".into(),
                value: ":".into(),
            },
            EnvVar {
                key: "name".into(),
                value: "bar".into(),
            },
            EnvVar {
                key: "out".into(),
                value: "/nix/store/4q0pg5zpfmznxscq3avycvf9xdvx50n3-bar".into(),
            },
            EnvVar {
                key: "outputHash".into(),
                value: "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba".into(),
            },
            EnvVar {
                key: "outputHashAlgo".into(),
                value: "sha256".into(),
            },
            EnvVar {
                key: "outputHashMode".into(),
                value: "recursive".into(),
            },
            EnvVar {
                key: "system".into(),
                value: ":".into(),
            },
        ];

        expected_environment_vars.extend(NIX_ENVIRONMENT_VARS.iter().map(|(k, v)| EnvVar {
            key: k.to_string(),
            value: Bytes::from_static(v.as_bytes()),
        }));

        expected_environment_vars.sort_unstable_by_key(|e| e.key.to_owned());

        assert_eq!(
            BuildRequest {
                command_args: vec![":".to_string()],
                outputs: vec!["nix/store/4q0pg5zpfmznxscq3avycvf9xdvx50n3-bar".into()],
                environment_vars: expected_environment_vars,
                inputs: vec![],
                inputs_dir: nix_compat::store_path::STORE_DIR.into(),
                constraints: Some(BuildConstraints {
                    system: derivation.system.clone(),
                    min_memory: 0,
                    network_access: true,
                    available_ro_paths: vec![],
                    provide_bin_sh: true,
                }),
                additional_files: vec![],
                working_dir: "build".into(),
                scratch_paths: vec!["build".into(), "nix/store".into()],
            },
            build_request
        );
    }
}
