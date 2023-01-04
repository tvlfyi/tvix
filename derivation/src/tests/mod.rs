use crate::derivation::Derivation;
use crate::output::Output;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use test_case::test_case;
use test_generator::test_resources;
use tvix_store::nixpath::NixPath;

const RESOURCES_PATHS: &str = "src/tests/derivation_tests";

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
fn validate(path_to_drv_file: &str) {
    let data = read_file(&format!("{}.json", path_to_drv_file));
    let derivation: Derivation = serde_json::from_str(&data).expect("JSON was not well-formatted");

    derivation
        .validate()
        .expect("derivation failed to validate")
}

#[test_resources("src/tests/derivation_tests/*.drv")]
fn check_to_string(path_to_drv_file: &str) {
    let data = read_file(&format!("{}.json", path_to_drv_file));
    let derivation: Derivation = serde_json::from_str(&data).expect("JSON was not well-formatted");

    let expected = read_file(path_to_drv_file);

    assert_eq!(expected, derivation.to_string());
}

#[test_case("bar","0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv"; "fixed_sha256")]
#[test_case("foo", "4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv"; "simple-sha256")]
#[test_case("bar", "ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv"; "fixed-sha1")]
#[test_case("foo", "ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv"; "simple-sha1")]
#[test_case("has-multi-out", "h32dahq0bx5rp1krcdx3a53asj21jvhk-has-multi-out.drv"; "multiple-outputs")]
#[test_case("structured-attrs", "9lj1lkjm2ag622mh4h9rpy6j607an8g2-structured-attrs.drv"; "structured-attrs")]
#[test_case("unicode", "52a9id8hx688hvlnz4d1n25ml1jdykz0-unicode.drv"; "unicode")]
fn derivation_path(name: &str, expected_path: &str) {
    let data = read_file(&format!("{}/{}.json", RESOURCES_PATHS, expected_path));
    let derivation: Derivation = serde_json::from_str(&data).expect("JSON was not well-formatted");

    assert_eq!(
        derivation.calculate_derivation_path(name).unwrap(),
        NixPath::from_string(expected_path).unwrap()
    );
}

/// This trims all outputs from a Derivation struct,
/// by setting outputs[$outputName].path and environment[$outputName] to the empty string.
fn derivation_with_trimmed_outputs(derivation: &Derivation) -> Derivation {
    let mut trimmed_env = derivation.environment.clone();
    let mut trimmed_outputs = derivation.outputs.clone();

    for (output_name, output) in &derivation.outputs {
        trimmed_env.insert(output_name.clone(), "".to_string());
        assert!(trimmed_outputs.contains_key(output_name));
        trimmed_outputs.insert(
            output_name.to_string(),
            Output {
                path: "".to_string(),
                ..output.clone()
            },
        );
    }

    // replace environment and outputs with the trimmed variants
    Derivation {
        environment: trimmed_env,
        outputs: trimmed_outputs,
        ..derivation.clone()
    }
}

#[test_case("0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv", "724f3e3634fce4cbbbd3483287b8798588e80280660b9a63fd13a1bc90485b33"; "fixed_sha256")]
#[test_case("ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv", "c79aebd0ce3269393d4a1fde2cbd1d975d879b40f0bf40a48f550edc107fd5df";"fixed-sha1")]
fn replacement_drv_path(drv_path: &str, expected_replacement_str: &str) {
    // read in the fixture
    let data = read_file(&format!("{}/{}.json", RESOURCES_PATHS, drv_path));
    let drv: Derivation = serde_json::from_str(&data).expect("must deserialize");

    let drv_replacement_str = drv.calculate_drv_replacement_str(|_| panic!("must not be called"));

    assert_eq!(expected_replacement_str, drv_replacement_str);
}

#[test_case("bar","0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv"; "fixed_sha256")]
#[test_case("foo", "4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv"; "simple-sha256")]
#[test_case("bar", "ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv"; "fixed-sha1")]
#[test_case("foo", "ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv"; "simple-sha1")]
#[test_case("has-multi-out", "h32dahq0bx5rp1krcdx3a53asj21jvhk-has-multi-out.drv"; "multiple-outputs")]
#[test_case("structured-attrs", "9lj1lkjm2ag622mh4h9rpy6j607an8g2-structured-attrs.drv"; "structured-attrs")]
#[test_case("unicode", "52a9id8hx688hvlnz4d1n25ml1jdykz0-unicode.drv"; "unicode")]
fn output_paths(name: &str, drv_path: &str) {
    // read in the fixture
    let data = read_file(&format!("{}/{}.json", RESOURCES_PATHS, drv_path));
    let expected_derivation: Derivation = serde_json::from_str(&data).expect("must deserialize");

    let mut derivation = derivation_with_trimmed_outputs(&expected_derivation);

    // calculate the drv replacement string.
    // We don't expect the lookup function to be called for most derivations.
    let replacement_str = derivation.calculate_drv_replacement_str(|drv_name| {
        // 4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv may lookup /nix/store/0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv
        // ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv may lookup /nix/store/ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv
        if name == "foo"
            && ((drv_path == "4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv"
                && drv_name == "/nix/store/0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv")
                || (drv_path == "ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv"
                    && drv_name == "/nix/store/ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv"))
        {
            // do the lookup, by reading in the fixture of the requested
            // drv_name, and calculating its drv replacement (on the non-stripped version)
            // In a real-world scenario you would have already done this during construction.

            let data = read_file(&format!(
                "{}/{}.json",
                RESOURCES_PATHS,
                Path::new(drv_name).file_name().unwrap().to_string_lossy()
            ));

            let drv: Derivation = serde_json::from_str(&data).expect("must deserialize");

            // calculate replacement string. These don't trigger any subsequent requests, as they're both FOD.
            drv.calculate_drv_replacement_str(|_| panic!("must not lookup"))
        } else {
            // we only expect this to be called in the "foo" testcase, for the "bar derivations"
            panic!("may only be called for foo testcase on bar derivations");
        }
    });

    // We need to calculate the replacement_str, as fixed-sha1 does use it.
    derivation
        .calculate_output_paths(&name, &replacement_str)
        .unwrap();

    // The derivation should now look like it was before
    assert_eq!(expected_derivation, derivation);
}
