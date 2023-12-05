use bstr::B;

use super::*;

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
        matches!(attrs, NixAttrs(AttrsRep::Im(_))),
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
        NixAttrs(AttrsRep::KV { name, value })
            if name.to_str().unwrap() == meaning_val.to_str().unwrap()
                || value.to_str().unwrap() == forty_two_val.to_str().unwrap() => {}

        _ => panic!(
            "K/V attribute set should use optimised representation, but got {:?}",
            kv_attrs
        ),
    }
}

#[test]
fn test_empty_attrs_iter() {
    let attrs = NixAttrs::construct(0, vec![]).unwrap();
    assert!(attrs.iter().next().is_none());
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

    let mut iter = kv_attrs.iter().collect::<Vec<_>>().into_iter();
    let (k, v) = iter.next().unwrap();
    assert!(k == *NAME_REF);
    assert!(v.to_str().unwrap() == meaning_val.to_str().unwrap());
    let (k, v) = iter.next().unwrap();
    assert!(k == *VALUE_REF);
    assert!(v.as_int().unwrap() == forty_two_val.as_int().unwrap());
    assert!(iter.next().is_none());
}

#[test]
fn test_map_attrs_iter() {
    let attrs = NixAttrs::construct(
        1,
        vec![Value::String("key".into()), Value::String("value".into())],
    )
    .expect("simple attr construction should succeed");

    let mut iter = attrs.iter().collect::<Vec<_>>().into_iter();
    let (k, v) = iter.next().unwrap();
    assert!(k == &NixString::from("key"));
    assert_eq!(v.to_str().unwrap(), B("value"));
    assert!(iter.next().is_none());
}
