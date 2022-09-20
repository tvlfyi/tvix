//! Support for configurable generation of arbitrary nix values

use proptest::{prelude::*, strategy::BoxedStrategy};
use std::{ffi::OsString, rc::Rc};

use super::{NixAttrs, NixList, NixString, Value};

#[derive(Clone)]
pub enum Parameters {
    Strategy(BoxedStrategy<Value>),
    Parameters {
        generate_internal_values: bool,
        generate_functions: bool,
        generate_nested: bool,
    },
}

impl Default for Parameters {
    fn default() -> Self {
        Self::Parameters {
            generate_internal_values: false,
            generate_functions: false,
            generate_nested: true,
        }
    }
}

impl Arbitrary for Value {
    type Parameters = Parameters;
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        match args {
            Parameters::Strategy(s) => s,
            Parameters::Parameters {
                generate_internal_values,
                generate_functions,
                generate_nested,
            } => {
                if generate_internal_values || generate_functions {
                    todo!("Generating internal values and functions not implemented yet")
                } else if generate_nested {
                    non_internal_value().boxed()
                } else {
                    leaf_value().boxed()
                }
            }
        }
    }
}

fn leaf_value() -> impl Strategy<Value = Value> {
    use Value::*;

    prop_oneof![
        Just(Null),
        any::<bool>().prop_map(Bool),
        any::<i64>().prop_map(Integer),
        any::<f64>().prop_map(Float),
        any::<NixString>().prop_map(String),
        any::<OsString>().prop_map(|s| Path(s.into())),
    ]
}

fn non_internal_value() -> impl Strategy<Value = Value> {
    leaf_value().prop_recursive(3, 5, 5, |inner| {
        prop_oneof![
            any_with::<NixAttrs>((
                Default::default(),
                Default::default(),
                Parameters::Strategy(inner.clone())
            ))
            .prop_map(|a| Value::Attrs(Rc::new(a))),
            any_with::<NixList>((Default::default(), Parameters::Strategy(inner)))
                .prop_map(Value::List)
        ]
    })
}
