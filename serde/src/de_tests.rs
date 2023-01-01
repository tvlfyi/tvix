use serde::Deserialize;
use std::collections::HashMap;

use crate::de::from_str;

#[test]
fn deserialize_none() {
    let result: Option<usize> = from_str("null").expect("should deserialize");
    assert_eq!(None, result);
}

#[test]
fn deserialize_some() {
    let result: Option<usize> = from_str("40 + 2").expect("should deserialize");
    assert_eq!(Some(42), result);
}

#[test]
fn deserialize_string() {
    let result: String = from_str(
        r#"
      let greeter = name: "Hello ${name}!";
      in greeter "Slartibartfast"
    "#,
    )
    .expect("should deserialize");

    assert_eq!(result, "Hello Slartibartfast!");
}

#[test]
fn deserialize_empty_list() {
    let result: Vec<usize> = from_str("[ ]").expect("should deserialize");
    assert!(result.is_empty())
}

#[test]
fn deserialize_integer_list() {
    let result: Vec<usize> =
        from_str("builtins.map (n: n + 2) [ 21 40 67 ]").expect("should deserialize");
    assert_eq!(result, vec![23, 42, 69]);
}

#[test]
fn deserialize_empty_map() {
    let result: HashMap<String, usize> = from_str("{ }").expect("should deserialize");
    assert!(result.is_empty());
}

#[test]
fn deserialize_integer_map() {
    let result: HashMap<String, usize> = from_str("{ age = 40 + 2; }").expect("should deserialize");
    assert_eq!(result.len(), 1);
    assert_eq!(*result.get("age").unwrap(), 42);
}

#[test]
fn deserialize_struct() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Person {
        name: String,
        age: usize,
    }

    let result: Person = from_str(
        r#"
    {
      name = "Slartibartfast";
      age = 42;
    }
    "#,
    )
    .expect("should deserialize");

    assert_eq!(
        result,
        Person {
            name: "Slartibartfast".into(),
            age: 42,
        }
    );
}

#[test]
fn deserialize_newtype() {
    #[derive(Debug, Deserialize, PartialEq)]
    struct Number(usize);

    let result: Number = from_str("42").expect("should deserialize");
    assert_eq!(result, Number(42));
}

#[test]
fn deserialize_tuple() {
    let result: (String, usize) = from_str(r#" [ "foo" 42 ] "#).expect("should deserialize");
    assert_eq!(result, ("foo".into(), 42));
}
