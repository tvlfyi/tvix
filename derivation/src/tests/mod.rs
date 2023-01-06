use crate::derivation::Derivation;
use crate::output::{Hash, Output};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use test_case::test_case;
use test_generator::test_resources;
use tvix_store::store_path::StorePath;

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
        StorePath::from_string(expected_path).unwrap()
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

/// Exercises the output path calculation functions like a constructing client
/// (an implementation of builtins.derivation) would do:
///
/// ```nix
/// rec {
///   bar = builtins.derivation {
///     name = "bar";
///     builder = ":";
///     system = ":";
///     outputHash = "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba";
///     outputHashAlgo = "sha256";
///     outputHashMode = "recursive";
///   };
///
///   foo = builtins.derivation {
///     name = "foo";
///     builder = ":";
///     system = ":";
///     inherit bar;
///   };
/// }
/// ```
/// It first assembles the bar derivation, does the output path calculation on
/// it, then continues with the foo derivation.
///
/// The code ensures the resulting Derivations match our fixtures.
#[test]
fn output_path_construction() {
    // create the bar derivation
    let mut bar_drv = Derivation {
        builder: ":".to_string(),
        system: ":".to_string(),
        ..Default::default()
    };

    // assemble bar env
    let bar_env = &mut bar_drv.environment;
    bar_env.insert("builder".to_string(), ":".to_string());
    bar_env.insert("name".to_string(), "bar".to_string());
    bar_env.insert("out".to_string(), "".to_string()); // will be calculated
    bar_env.insert(
        "outputHash".to_string(),
        "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba".to_string(),
    );
    bar_env.insert("outputHashAlgo".to_string(), "sha256".to_string());
    bar_env.insert("outputHashMode".to_string(), "recursive".to_string());
    bar_env.insert("system".to_string(), ":".to_string());

    // assemble bar outputs
    bar_drv.outputs.insert(
        "out".to_string(),
        Output {
            path: "".to_string(), // will be calculated
            hash: Some(Hash {
                digest: "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba"
                    .to_string(),
                algo: "r:sha256".to_string(),
            }),
        },
    );

    // calculate bar output paths
    let bar_calc_result = bar_drv.calculate_output_paths(
        "bar",
        &bar_drv.calculate_drv_replacement_str(|_| panic!("is FOD, should not lookup")),
    );
    assert!(bar_calc_result.is_ok());

    // ensure it matches our bar fixture
    let bar_data = read_file(&format!(
        "{}/{}.json",
        RESOURCES_PATHS, "0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv"
    ));
    let bar_drv_expected: Derivation = serde_json::from_str(&bar_data).expect("must deserialize");
    assert_eq!(bar_drv_expected, bar_drv);

    // now construct foo, which requires bar_drv
    // Note how we refer to the output path, drv name and replacement_str (with calculated output paths) of bar.
    let bar_output_path = &bar_drv.outputs.get("out").expect("must exist").path;
    let bar_drv_replacement_str =
        &bar_drv.calculate_drv_replacement_str(|_| panic!("is FOD, should not lookup"));

    let bar_drv_path = bar_drv
        .calculate_derivation_path("bar")
        .expect("must succeed");

    // create foo derivation
    let mut foo_drv = Derivation {
        builder: ":".to_string(),
        system: ":".to_string(),
        ..Default::default()
    };

    // assemble foo env
    let foo_env = &mut foo_drv.environment;
    foo_env.insert("bar".to_string(), bar_output_path.to_string());
    foo_env.insert("builder".to_string(), ":".to_string());
    foo_env.insert("name".to_string(), "foo".to_string());
    foo_env.insert("out".to_string(), "".to_string()); // will be calculated
    foo_env.insert("system".to_string(), ":".to_string());

    // asssemble foo outputs
    foo_drv.outputs.insert(
        "out".to_string(),
        Output {
            path: "".to_string(), // will be calculated
            hash: None,
        },
    );

    // assemble foo input_derivations
    foo_drv
        .input_derivations
        .insert(bar_drv_path.to_absolute_path(), vec!["out".to_string()]);

    // calculate foo output paths
    let foo_calc_result = foo_drv.calculate_output_paths(
        "foo",
        &foo_drv.calculate_drv_replacement_str(|drv_name| {
            if drv_name != "/nix/store/0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv" {
                panic!("lookup called with unexpected drv_name: {}", drv_name);
            }
            bar_drv_replacement_str.clone()
        }),
    );
    assert!(foo_calc_result.is_ok());

    // ensure it matches our foo fixture
    let foo_data = read_file(&format!(
        "{}/{}.json",
        RESOURCES_PATHS, "4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv",
    ));
    let foo_drv_expected: Derivation = serde_json::from_str(&foo_data).expect("must deserialize");
    assert_eq!(foo_drv_expected, foo_drv);

    assert_eq!(
        StorePath::from_string("4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv").expect("must succeed"),
        foo_drv
            .calculate_derivation_path("foo")
            .expect("must succeed")
    );
}
