//! Implements `builtins.derivation`, the core of what makes Nix build packages.

use std::collections::{btree_map, BTreeSet};
use tvix_derivation::{Derivation, Hash};
use tvix_eval::{AddContext, CoercionKind, ErrorKind, NixList, Value, VM};

use crate::errors::Error;
use crate::known_paths::{KnownPaths, PathType};

// Constants used for strangely named fields in derivation inputs.
const IGNORE_NULLS: &str = "__ignoreNulls";

/// Helper function for populating the `drv.outputs` field from a
/// manually specified set of outputs, instead of the default
/// `outputs`.
fn populate_outputs(vm: &mut VM, drv: &mut Derivation, outputs: NixList) -> Result<(), ErrorKind> {
    // Remove the original default `out` output.
    drv.outputs.clear();

    for output in outputs {
        let output_name = output
            .force(vm)?
            .to_str()
            .context("determining output name")?;

        if drv
            .outputs
            .insert(output_name.as_str().into(), Default::default())
            .is_some()
        {
            return Err(Error::DuplicateOutput(output_name.as_str().into()).into());
        }
    }

    Ok(())
}

/// Populate the inputs of a derivation from the build references
/// found when scanning the derivation's parameters.
fn populate_inputs<I: IntoIterator<Item = String>>(
    drv: &mut Derivation,
    known_paths: &KnownPaths,
    references: I,
) {
    for reference in references.into_iter() {
        match &known_paths[&reference] {
            PathType::Plain => {
                drv.input_sources.insert(reference.to_string());
            }

            PathType::Output { name, derivation } => {
                match drv.input_derivations.entry(derivation.clone()) {
                    btree_map::Entry::Vacant(entry) => {
                        entry.insert(BTreeSet::from([name.clone()]));
                    }

                    btree_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().insert(name.clone());
                    }
                }
            }

            PathType::Derivation { output_names } => {
                match drv.input_derivations.entry(reference.to_string()) {
                    btree_map::Entry::Vacant(entry) => {
                        entry.insert(output_names.clone());
                    }

                    btree_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().extend(output_names.clone().into_iter());
                    }
                }
            }
        }
    }
}

/// Populate the output configuration of a derivation based on the
/// parameters passed to the call, flipping the required
/// parameters for a fixed-output derivation if necessary.
///
/// This function handles all possible combinations of the
/// parameters, including invalid ones.
fn populate_output_configuration(
    drv: &mut Derivation,
    vm: &mut VM,
    hash: Option<&Value>,      // in nix: outputHash
    hash_algo: Option<&Value>, // in nix: outputHashAlgo
    hash_mode: Option<&Value>, // in nix: outputHashmode
) -> Result<(), ErrorKind> {
    match (hash, hash_algo, hash_mode) {
        (Some(hash), Some(algo), hash_mode) => match drv.outputs.get_mut("out") {
            None => return Err(Error::ConflictingOutputTypes.into()),
            Some(out) => {
                let algo = algo
                    .force(vm)?
                    .coerce_to_string(CoercionKind::Strong, vm)?
                    .as_str()
                    .to_string();

                let hash_mode = match hash_mode {
                    None => None,
                    Some(mode) => Some(
                        mode.force(vm)?
                            .coerce_to_string(CoercionKind::Strong, vm)?
                            .as_str()
                            .to_string(),
                    ),
                };

                let algo = match hash_mode.as_deref() {
                    None | Some("flat") => algo,
                    Some("recursive") => format!("r:{}", algo),
                    Some(other) => {
                        return Err(Error::InvalidOutputHashMode(other.to_string()).into())
                    }
                };

                out.hash = Some(Hash {
                    algo,

                    digest: hash
                        .force(vm)?
                        .coerce_to_string(CoercionKind::Strong, vm)?
                        .as_str()
                        .to_string(),
                });
            }
        },

        _ => {}
    }

    Ok(())
}

/// Handles derivation parameters which are not just forwarded to
/// the environment. The return value indicates whether the
/// parameter should be included in the environment.
fn handle_derivation_parameters(
    drv: &mut Derivation,
    vm: &mut VM,
    name: &str,
    value: &Value,
    val_str: &str,
) -> Result<bool, ErrorKind> {
    match name {
        IGNORE_NULLS => return Ok(false),

        // Command line arguments to the builder.
        "args" => {
            let args = value.to_list()?;
            for arg in args {
                drv.arguments.push(
                    arg.force(vm)?
                        .coerce_to_string(CoercionKind::Strong, vm)
                        .context("handling command-line builder arguments")?
                        .as_str()
                        .to_string(),
                );
            }

            // The arguments do not appear in the environment.
            return Ok(false);
        }

        // Explicitly specified drv outputs (instead of default [ "out" ])
        "outputs" => {
            let outputs = value
                .to_list()
                .context("looking at the `outputs` parameter of the derivation")?;

            drv.outputs.clear();
            populate_outputs(vm, drv, outputs)?;
        }

        "builder" => {
            drv.builder = val_str.to_string();
        }

        "system" => {
            drv.system = val_str.to_string();
        }

        _ => {}
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tvix_eval::observer::NoOpObserver;
    use tvix_eval::Value;

    static mut OBSERVER: NoOpObserver = NoOpObserver {};

    // Creates a fake VM for tests, which can *not* actually be
    // used to force (most) values but can satisfy the type
    // parameter.
    fn fake_vm() -> VM<'static> {
        // safe because accessing the observer doesn't actually do anything
        unsafe {
            VM::new(
                Default::default(),
                Box::new(tvix_eval::DummyIO),
                &mut OBSERVER,
                Default::default(),
            )
        }
    }

    #[test]
    fn populate_outputs_ok() {
        let mut vm = fake_vm();
        let mut drv = Derivation::default();
        drv.outputs.insert("out".to_string(), Default::default());

        let outputs = NixList::construct(
            2,
            vec![Value::String("foo".into()), Value::String("bar".into())],
        );

        populate_outputs(&mut vm, &mut drv, outputs).expect("populate_outputs should succeed");

        assert_eq!(drv.outputs.len(), 2);
        assert!(drv.outputs.contains_key("bar"));
        assert!(drv.outputs.contains_key("foo"));
    }

    #[test]
    fn populate_outputs_duplicate() {
        let mut vm = fake_vm();
        let mut drv = Derivation::default();
        drv.outputs.insert("out".to_string(), Default::default());

        let outputs = NixList::construct(
            2,
            vec![Value::String("foo".into()), Value::String("foo".into())],
        );

        populate_outputs(&mut vm, &mut drv, outputs)
            .expect_err("supplying duplicate outputs should fail");
    }

    #[test]
    fn populate_inputs_empty() {
        let mut drv = Derivation::default();
        let paths = KnownPaths::default();
        let inputs = vec![];

        populate_inputs(&mut drv, &paths, inputs);

        assert!(drv.input_sources.is_empty());
        assert!(drv.input_derivations.is_empty());
    }

    #[test]
    fn populate_inputs_all() {
        let mut drv = Derivation::default();

        let mut paths = KnownPaths::default();
        paths.plain("/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-foo");
        paths.drv(
            "/nix/store/aqffiyqx602lbam7n1zsaz3yrh6v08pc-bar.drv",
            &["out"],
        );
        paths.output(
            "/nix/store/zvpskvjwi72fjxg0vzq822sfvq20mq4l-bar",
            "out",
            "/nix/store/aqffiyqx602lbam7n1zsaz3yrh6v08pc-bar.drv",
        );

        let inputs: Vec<String> = vec![
            "/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-foo".into(),
            "/nix/store/aqffiyqx602lbam7n1zsaz3yrh6v08pc-bar.drv".into(),
            "/nix/store/zvpskvjwi72fjxg0vzq822sfvq20mq4l-bar".into(),
        ];

        populate_inputs(&mut drv, &paths, inputs);

        assert_eq!(drv.input_sources.len(), 1);
        assert!(drv
            .input_sources
            .contains("/nix/store/fn7zvafq26f0c8b17brs7s95s10ibfzs-foo"));

        assert_eq!(drv.input_derivations.len(), 1);
        assert!(drv
            .input_derivations
            .contains_key("/nix/store/aqffiyqx602lbam7n1zsaz3yrh6v08pc-bar.drv"));
    }

    #[test]
    fn populate_output_config_std() {
        let mut vm = fake_vm();
        let mut drv = Derivation::default();

        populate_output_configuration(&mut drv, &mut vm, None, None, None)
            .expect("populate_output_configuration() should succeed");

        assert_eq!(drv, Derivation::default(), "derivation should be unchanged");
    }

    #[test]
    fn populate_output_config_fod() {
        let mut vm = fake_vm();
        let mut drv = Derivation::default();
        drv.outputs.insert("out".to_string(), Default::default());

        let hash = Value::String(
            "0000000000000000000000000000000000000000000000000000000000000000".into(),
        );

        let algo = Value::String("sha256".into());

        populate_output_configuration(&mut drv, &mut vm, Some(&hash), Some(&algo), None)
            .expect("populate_output_configuration() should succeed");

        let expected = Hash {
            algo: "sha256".into(),
            digest: "0000000000000000000000000000000000000000000000000000000000000000".into(),
        };

        assert_eq!(drv.outputs["out"].hash, Some(expected));
    }

    #[test]
    fn populate_output_config_fod_recursive() {
        let mut vm = fake_vm();
        let mut drv = Derivation::default();
        drv.outputs.insert("out".to_string(), Default::default());

        let hash = Value::String(
            "0000000000000000000000000000000000000000000000000000000000000000".into(),
        );

        let algo = Value::String("sha256".into());
        let mode = Value::String("recursive".into());

        populate_output_configuration(&mut drv, &mut vm, Some(&hash), Some(&algo), Some(&mode))
            .expect("populate_output_configuration() should succeed");

        let expected = Hash {
            algo: "r:sha256".into(),
            digest: "0000000000000000000000000000000000000000000000000000000000000000".into(),
        };

        assert_eq!(drv.outputs["out"].hash, Some(expected));
    }

    #[test]
    fn handle_outputs_parameter() {
        let mut vm = fake_vm();
        let mut drv = Derivation::default();
        drv.outputs.insert("out".to_string(), Default::default());

        let outputs = Value::List(NixList::construct(
            2,
            vec![Value::String("foo".into()), Value::String("bar".into())],
        ));
        let outputs_str = outputs
            .coerce_to_string(CoercionKind::Strong, &mut vm)
            .unwrap();

        handle_derivation_parameters(&mut drv, &mut vm, "outputs", &outputs, outputs_str.as_str())
            .expect("handling 'outputs' parameter should succeed");

        assert_eq!(drv.outputs.len(), 2);
        assert!(drv.outputs.contains_key("bar"));
        assert!(drv.outputs.contains_key("foo"));
    }

    #[test]
    fn handle_args_parameter() {
        let mut vm = fake_vm();
        let mut drv = Derivation::default();

        let args = Value::List(NixList::construct(
            3,
            vec![
                Value::String("--foo".into()),
                Value::String("42".into()),
                Value::String("--bar".into()),
            ],
        ));

        let args_str = args
            .coerce_to_string(CoercionKind::Strong, &mut vm)
            .unwrap();

        handle_derivation_parameters(&mut drv, &mut vm, "args", &args, args_str.as_str())
            .expect("handling 'args' parameter should succeed");

        assert_eq!(
            drv.arguments,
            vec!["--foo".to_string(), "42".to_string(), "--bar".to_string()]
        );
    }
}
