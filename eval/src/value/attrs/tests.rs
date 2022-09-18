use super::*;

mod nix_eq {
    use crate::observer::NoOpObserver;

    use super::*;
    use proptest::prelude::ProptestConfig;
    use test_strategy::proptest;

    #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
    fn reflexive(x: NixAttrs) {
        let mut observer = NoOpObserver {};
        let mut vm = VM::new(&mut observer);

        assert!(x.nix_eq(&x, &mut vm).unwrap())
    }

    #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
    fn symmetric(x: NixAttrs, y: NixAttrs) {
        let mut observer = NoOpObserver {};
        let mut vm = VM::new(&mut observer);

        assert_eq!(
            x.nix_eq(&y, &mut vm).unwrap(),
            y.nix_eq(&x, &mut vm).unwrap()
        )
    }

    #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
    fn transitive(x: NixAttrs, y: NixAttrs, z: NixAttrs) {
        let mut observer = NoOpObserver {};
        let mut vm = VM::new(&mut observer);

        if x.nix_eq(&y, &mut vm).unwrap() && y.nix_eq(&z, &mut vm).unwrap() {
            assert!(x.nix_eq(&z, &mut vm).unwrap())
        }
    }
}

#[test]
fn test_empty_attrs() {
    let attrs = NixAttrs::construct(0, vec![]).expect("empty attr construction should succeed");

    assert!(
        matches!(attrs, NixAttrs(AttrsRep::Empty)),
        "empty attribute set should use optimised representation"
    );
}

#[test]
fn test_simple_attrs() {
    let attrs = NixAttrs::construct(
        1,
        vec![Value::String("key".into()), Value::String("value".into())],
    )
    .expect("simple attr construction should succeed");

    assert!(
        matches!(attrs, NixAttrs(AttrsRep::Map(_))),
        "simple attribute set should use map representation",
    )
}

#[test]
fn test_kv_attrs() {
    let name_val = Value::String("name".into());
    let value_val = Value::String("value".into());
    let meaning_val = Value::String("meaning".into());
    let forty_two_val = Value::Integer(42);

    let kv_attrs = NixAttrs::construct(
        2,
        vec![
            value_val,
            forty_two_val.clone(),
            name_val,
            meaning_val.clone(),
        ],
    )
    .expect("constructing K/V pair attrs should succeed");

    match kv_attrs {
        NixAttrs(AttrsRep::KV { name, value }) if name == meaning_val || value == forty_two_val => {
        }

        _ => panic!(
            "K/V attribute set should use optimised representation, but got {:?}",
            kv_attrs
        ),
    }
}

#[test]
fn test_empty_attrs_iter() {
    let attrs = NixAttrs::construct(0, vec![]).unwrap();
    assert_eq!(attrs.iter().next(), None);
}

#[test]
fn test_kv_attrs_iter() {
    let name_val = Value::String("name".into());
    let value_val = Value::String("value".into());
    let meaning_val = Value::String("meaning".into());
    let forty_two_val = Value::Integer(42);

    let kv_attrs = NixAttrs::construct(
        2,
        vec![
            value_val,
            forty_two_val.clone(),
            name_val,
            meaning_val.clone(),
        ],
    )
    .expect("constructing K/V pair attrs should succeed");

    assert_eq!(
        kv_attrs.iter().collect::<Vec<_>>(),
        vec![
            (NixString::NAME_REF, &meaning_val),
            (NixString::VALUE_REF, &forty_two_val)
        ]
    );
}

#[test]
fn test_map_attrs_iter() {
    let attrs = NixAttrs::construct(
        1,
        vec![Value::String("key".into()), Value::String("value".into())],
    )
    .expect("simple attr construction should succeed");

    assert_eq!(
        attrs.iter().collect::<Vec<_>>(),
        vec![(&NixString::from("key"), &Value::String("value".into()))],
    );
}
