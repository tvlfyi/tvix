//! Support for configurable generation of arbitrary nix values

use imbl::proptest::ord_map;
use proptest::collection::vec;
use proptest::{prelude::*, strategy::BoxedStrategy};
use std::ffi::OsString;

use super::{attrs::AttrsRep, NixAttrs, NixList, NixString, Value};

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

impl Arbitrary for NixAttrs {
    type Parameters = Parameters;
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        prop_oneof![
            // Empty attrs representation
            Just(Self(AttrsRep::Empty)),
            // KV representation (name/value pairs)
            (
                any_with::<Value>(args.clone()),
                any_with::<Value>(args.clone())
            )
                .prop_map(|(name, value)| Self(AttrsRep::KV { name, value })),
            // Map representation
            ord_map(NixString::arbitrary(), Value::arbitrary_with(args), 0..100)
                .prop_map(|map| Self(AttrsRep::Im(map)))
        ]
        .boxed()
    }
}

impl Arbitrary for NixList {
    type Parameters = <Value as Arbitrary>::Parameters;
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
        vec(<Value as Arbitrary>::arbitrary_with(args), 0..100)
            .prop_map(NixList::from)
            .boxed()
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
        any::<OsString>().prop_map(|s| Path(Box::new(s.into()))),
    ]
}

fn non_internal_value() -> impl Strategy<Value = Value> {
    leaf_value().prop_recursive(3, 5, 5, |inner| {
        prop_oneof![
            NixAttrs::arbitrary_with(Parameters::Strategy(inner.clone())).prop_map(Value::attrs),
            any_with::<NixList>(Parameters::Strategy(inner)).prop_map(Value::List)
        ]
    })
}
