//! Implements `builtins.derivation`, the core of what makes Nix build packages.

use tvix_derivation::Derivation;
use tvix_eval::{AddContext, ErrorKind, NixList, VM};

use crate::errors::Error;

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
}
