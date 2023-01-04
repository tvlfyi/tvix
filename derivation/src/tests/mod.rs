use crate::derivation::Derivation;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use test_generator::test_resources;

fn read_file(path: &str) -> String {
    let path = Path::new(path);
    let mut file = File::open(path).unwrap();
    let mut data = String::new();

    file.read_to_string(&mut data).unwrap();

    return data;
}

#[test_resources("src/tests/derivation_tests/*.drv")]
fn check_serizaliation(path_to_drv_file: &str) {
    let data = read_file(&format!("{}.json", path_to_drv_file));
    let derivation: Derivation = serde_json::from_str(&data).expect("JSON was not well-formatted");

    let mut serialized_derivation = String::new();
    derivation.serialize(&mut serialized_derivation).unwrap();

    let expected = read_file(path_to_drv_file);

    assert_eq!(expected, serialized_derivation);
}

#[test_resources("src/tests/derivation_tests/*.drv")]
fn check_to_string(path_to_drv_file: &str) {
    let data = read_file(&format!("{}.json", path_to_drv_file));
    let derivation: Derivation = serde_json::from_str(&data).expect("JSON was not well-formatted");

    let expected = read_file(path_to_drv_file);

    assert_eq!(expected, derivation.to_string());
}
