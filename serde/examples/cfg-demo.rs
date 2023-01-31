//! This program demonstrates how to use tvix_serde to deserialise
//! program configuration (or other data) from Nix code.
//!
//! This makes it possible to use Nix as an embedded config language.
//! For greater control over evaluation, and for features like adding
//! additional builtins, depending directly on tvix_eval would be
//! required.
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
enum Flavour {
    Tasty,
    Okay,
    Eww,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Data {
    name: String,
    foods: HashMap<String, Flavour>,
}

fn main() {
    // Get the content from wherever, read it from a file, receive it
    // over the network - whatever floats your boat! We'll include it
    // as a string.
    let code = include_str!("foods.nix");

    // Now you can use tvix_serde to deserialise the struct:
    let foods: Data = tvix_serde::from_str(code).expect("deserialisation should succeed");

    println!("These are the foods:\n{:#?}", foods);
}
