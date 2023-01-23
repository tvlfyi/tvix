//! Implements `builtins.derivation`, the core of what makes Nix build packages.

use std::cell::RefCell;
use std::collections::{btree_map, BTreeSet};
use std::rc::Rc;
use tvix_derivation::{Derivation, Hash};
use tvix_eval::builtin_macros::builtins;
use tvix_eval::{AddContext, CoercionKind, ErrorKind, NixAttrs, NixList, Value, VM};

use crate::errors::Error;
use crate::known_paths::{KnownPaths, PathType};

// Constants used for strangely named fields in derivation inputs.
const STRUCTURED_ATTRS: &str = "__structuredAttrs";
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

#[builtins(state = "Rc<RefCell<KnownPaths>>")]
mod derivation_builtins {
    use super::*;

    /// Strictly construct a Nix derivation from the supplied arguments.
    ///
    /// This is considered an internal function, users usually want to
    /// use the higher-level `builtins.derivation` instead.
    #[builtin("derivationStrict")]
    fn builtin_derivation_strict(
        state: Rc<RefCell<KnownPaths>>,
        vm: &mut VM,
        input: Value,
    ) -> Result<Value, ErrorKind> {
        let input = input.to_attrs()?;
        let name = input
            .select_required("name")?
            .force(vm)?
            .to_str()
            .context("determining derivation name")?;

        // Check whether attributes should be passed as a JSON file.
        // TODO: the JSON serialisation has to happen here.
        if let Some(sa) = input.select(STRUCTURED_ATTRS) {
            if sa.force(vm)?.as_bool()? {
                return Err(ErrorKind::NotImplemented(STRUCTURED_ATTRS));
            }
        }

        // Check whether null attributes should be ignored or passed through.
        let ignore_nulls = match input.select(IGNORE_NULLS) {
            Some(b) => b.force(vm)?.as_bool()?,
            None => false,
        };

        let mut drv = Derivation::default();
        drv.outputs.insert("out".to_string(), Default::default());

        // Configure fixed-output derivations if required.
        populate_output_configuration(
            &mut drv,
            vm,
            input.select("outputHash"),
            input.select("outputHashAlgo"),
            input.select("outputHashMode"),
        )?;

        for (name, value) in input.into_iter_sorted() {
            if ignore_nulls && matches!(*value.force(vm)?, Value::Null) {
                continue;
            }

            let val_str = value
                .force(vm)?
                .coerce_to_string(CoercionKind::Strong, vm)?
                .as_str()
                .to_string();

            // handle_derivation_parameters tells us whether the
            // argument should be added to the environment; continue
            // to the next one otherwise
            if !handle_derivation_parameters(&mut drv, vm, name.as_str(), &value, &val_str)? {
                continue;
            }

            // Most of these are also added to the builder's environment in "raw" form.
            if drv
                .environment
                .insert(name.as_str().to_string(), val_str)
                .is_some()
            {
                return Err(Error::DuplicateEnvVar(name.as_str().to_string()).into());
            }
        }

        // Scan references in relevant attributes to detect any build-references.
        let mut refscan = state.borrow().reference_scanner();
        drv.arguments.iter().for_each(|s| refscan.scan_str(s));
        drv.environment.values().for_each(|s| refscan.scan_str(s));
        refscan.scan_str(&drv.builder);

        // Each output name needs to exist in the environment, at this
        // point initialised as an empty string because that is the
        // way of Golang ;)
        for output in drv.outputs.keys() {
            if drv
                .environment
                .insert(output.to_string(), String::new())
                .is_some()
            {
                return Err(Error::ShadowedOutput(output.to_string()).into());
            }
        }

        let mut known_paths = state.borrow_mut();
        populate_inputs(&mut drv, &known_paths, refscan.finalise());

        // At this point, derivation fields are fully populated from
        // eval data structures.
        drv.validate(false).map_err(Error::InvalidDerivation)?;

        let tmp_replacement_str =
            drv.calculate_drv_replacement_str(|drv| known_paths.get_replacement_string(drv));

        drv.calculate_output_paths(&name, &tmp_replacement_str)
            .map_err(Error::InvalidDerivation)?;

        let actual_replacement_str =
            drv.calculate_drv_replacement_str(|drv| known_paths.get_replacement_string(drv));

        let derivation_path = drv
            .calculate_derivation_path(&name)
            .map_err(Error::InvalidDerivation)?;

        known_paths
            .add_replacement_string(derivation_path.to_absolute_path(), &actual_replacement_str);

        // mark all the new paths as known
        let output_names: Vec<String> = drv.outputs.keys().map(Clone::clone).collect();
        known_paths.drv(derivation_path.to_absolute_path(), &output_names);

        for (output_name, output) in &drv.outputs {
            known_paths.output(
                &output.path,
                output_name,
                derivation_path.to_absolute_path(),
            );
        }

        let mut new_attrs: Vec<(String, String)> = drv
            .outputs
            .into_iter()
            .map(|(name, output)| (name, output.path))
            .collect();

        new_attrs.push(("drvPath".to_string(), derivation_path.to_absolute_path()));

        Ok(Value::Attrs(Box::new(NixAttrs::from_iter(
            new_attrs.into_iter(),
        ))))
    }

    #[builtin("toFile")]
    fn builtin_to_file(
        state: Rc<RefCell<KnownPaths>>,
        _: &mut VM,
        name: Value,
        content: Value,
    ) -> Result<Value, ErrorKind> {
        let name = name
            .to_str()
            .context("evaluating the `name` parameter of builtins.toFile")?;
        let content = content
            .to_str()
            .context("evaluating the `content` parameter of builtins.toFile")?;

        let mut refscan = state.borrow().reference_scanner();
        refscan.scan_str(content.as_str());
        let refs = refscan.finalise();

        // TODO: fail on derivation references (only "plain" is allowed here)

        let path = tvix_derivation::path_with_references(name.as_str(), content.as_str(), refs)
            .map_err(Error::InvalidDerivation)?
            .to_absolute_path();

        state.borrow_mut().plain(&path);

        // TODO: actually persist the file in the store at that path ...

        Ok(Value::String(path.into()))
    }
}

pub use derivation_builtins::builtins as derivation_builtins;

#[cfg(test)]
mod tests {
    use super::*;
    use tvix_eval::observer::NoOpObserver;

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
