//! Implements `builtins.derivation`, the core of what makes Nix build packages.

use std::collections::{btree_map, BTreeSet};
use tvix_derivation::Derivation;
use tvix_eval::{AddContext, ErrorKind, NixList, VM};

use crate::errors::Error;
use crate::known_paths::{KnownPaths, PathType};

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
}
