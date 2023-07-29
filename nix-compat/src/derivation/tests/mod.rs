use crate::derivation::output::Output;
use crate::derivation::Derivation;
use crate::nixhash::NixHash;
use crate::store_path::StorePath;
use bstr::{BStr, BString};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;
use test_case::test_case;
use test_generator::test_resources;

const RESOURCES_PATHS: &str = "src/derivation/tests/derivation_tests";

fn read_file(path: &str) -> BString {
    let path = Path::new(path);
    let mut file = File::open(path).unwrap();
    let mut data = Vec::new();

    file.read_to_end(&mut data).unwrap();

    data.into()
}

#[test_resources("src/derivation/tests/derivation_tests/*.drv")]
fn check_serizaliation(path_to_drv_file: &str) {
    // skip JSON files known to fail parsing
    if path_to_drv_file.ends_with("cp1252.drv") || path_to_drv_file.ends_with("latin1.drv") {
        return;
    }
    let data = read_file(&format!("{}.json", path_to_drv_file));
    let derivation: Derivation =
        serde_json::from_slice(&data).expect("JSON was not well-formatted");

    let mut serialized_derivation = Vec::new();
    derivation.serialize(&mut serialized_derivation).unwrap();

    let expected = read_file(path_to_drv_file);

    assert_eq!(expected, BStr::new(&serialized_derivation));
}

#[test_resources("src/derivation/tests/derivation_tests/*.drv")]
fn validate(path_to_drv_file: &str) {
    // skip JSON files known to fail parsing
    if path_to_drv_file.ends_with("cp1252.drv") || path_to_drv_file.ends_with("latin1.drv") {
        return;
    }
    let data = read_file(&format!("{}.json", path_to_drv_file));
    let derivation: Derivation =
        serde_json::from_slice(&data).expect("JSON was not well-formatted");

    derivation
        .validate(true)
        .expect("derivation failed to validate")
}

#[test_resources("src/derivation/tests/derivation_tests/*.drv")]
fn check_to_aterm_bytes(path_to_drv_file: &str) {
    // skip JSON files known to fail parsing
    if path_to_drv_file.ends_with("cp1252.drv") || path_to_drv_file.ends_with("latin1.drv") {
        return;
    }
    let data = read_file(&format!("{}.json", path_to_drv_file));
    let derivation: Derivation =
        serde_json::from_slice(&data).expect("JSON was not well-formatted");

    let expected = read_file(path_to_drv_file);

    assert_eq!(expected, BStr::new(&derivation.to_aterm_bytes()));
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
    let derivation: Derivation =
        serde_json::from_slice(&data).expect("JSON was not well-formatted");

    assert_eq!(
        derivation.calculate_derivation_path(name).unwrap(),
        StorePath::from_str(expected_path).unwrap()
    );
}

/// This trims all output paths from a Derivation struct,
/// by setting outputs[$outputName].path and environment[$outputName] to the empty string.
fn derivation_with_trimmed_output_paths(derivation: &Derivation) -> Derivation {
    let mut trimmed_env = derivation.environment.clone();
    let mut trimmed_outputs = derivation.outputs.clone();

    for (output_name, output) in &derivation.outputs {
        trimmed_env.insert(output_name.clone(), "".into());
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

#[test_case("0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv", "sha256:724f3e3634fce4cbbbd3483287b8798588e80280660b9a63fd13a1bc90485b33"; "fixed_sha256")]
#[test_case("ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv", "sha256:c79aebd0ce3269393d4a1fde2cbd1d975d879b40f0bf40a48f550edc107fd5df";"fixed-sha1")]
fn derivation_or_fod_hash(drv_path: &str, expected_nix_hash_string: &str) {
    // read in the fixture
    let data = read_file(&format!("{}/{}.json", RESOURCES_PATHS, drv_path));
    let drv: Derivation = serde_json::from_slice(&data).expect("must deserialize");

    let actual = drv.derivation_or_fod_hash(|_| panic!("must not be called"));

    assert_eq!(expected_nix_hash_string, actual.to_nix_hash_string());
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
    let expected_derivation: Derivation = serde_json::from_slice(&data).expect("must deserialize");

    let mut derivation = derivation_with_trimmed_output_paths(&expected_derivation);

    // calculate the derivation_or_fod_hash of derivation
    // We don't expect the lookup function to be called for most derivations.
    let calculated_derivation_or_fod_hash = derivation.derivation_or_fod_hash(|parent_drv_path| {
        // 4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv may lookup /nix/store/0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv
        // ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv may lookup /nix/store/ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv
        if name == "foo"
            && ((drv_path == "4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv"
                && parent_drv_path == "/nix/store/0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv")
                || (drv_path == "ch49594n9avinrf8ip0aslidkc4lxkqv-foo.drv"
                    && parent_drv_path == "/nix/store/ss2p4wmxijn652haqyd7dckxwl4c7hxx-bar.drv"))
        {
            // do the lookup, by reading in the fixture of the requested
            // drv_name, and calculating its drv replacement (on the non-stripped version)
            // In a real-world scenario you would have already done this during construction.

            let data = read_file(&format!(
                "{}/{}.json",
                RESOURCES_PATHS,
                Path::new(parent_drv_path)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
            ));

            let drv: Derivation = serde_json::from_slice(&data).expect("must deserialize");

            // calculate derivation_or_fod_hash for each parent.
            // This may not trigger subsequent requests, as both parents are FOD.
            drv.derivation_or_fod_hash(|_| panic!("must not lookup"))
        } else {
            // we only expect this to be called in the "foo" testcase, for the "bar derivations"
            panic!("may only be called for foo testcase on bar derivations");
        }
    });

    derivation
        .calculate_output_paths(name, &calculated_derivation_or_fod_hash)
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
    bar_env.insert("builder".to_string(), ":".into());
    bar_env.insert("name".to_string(), "bar".into());
    bar_env.insert("out".to_string(), "".into()); // will be calculated
    bar_env.insert(
        "outputHash".to_string(),
        "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba".into(),
    );
    bar_env.insert("outputHashAlgo".to_string(), "sha256".into());
    bar_env.insert("outputHashMode".to_string(), "recursive".into());
    bar_env.insert("system".to_string(), ":".into());

    // assemble bar outputs
    bar_drv.outputs.insert(
        "out".to_string(),
        Output {
            path: "".to_string(), // will be calculated
            hash_with_mode: Some(crate::nixhash::NixHashWithMode::Recursive(NixHash {
                digest: data_encoding::HEXLOWER
                    .decode(
                        "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba"
                            .as_bytes(),
                    )
                    .unwrap(),
                algo: crate::nixhash::HashAlgo::Sha256,
            })),
        },
    );

    // calculate bar output paths
    let bar_calc_result = bar_drv.calculate_output_paths(
        "bar",
        &bar_drv.derivation_or_fod_hash(|_| panic!("is FOD, should not lookup")),
    );
    assert!(bar_calc_result.is_ok());

    // ensure it matches our bar fixture
    let bar_data = read_file(&format!(
        "{}/{}.json",
        RESOURCES_PATHS, "0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv"
    ));
    let bar_drv_expected: Derivation = serde_json::from_slice(&bar_data).expect("must deserialize");
    assert_eq!(bar_drv_expected, bar_drv);

    // now construct foo, which requires bar_drv
    // Note how we refer to the output path, drv name and replacement_str (with calculated output paths) of bar.
    let bar_output_path = &bar_drv.outputs.get("out").expect("must exist").path;
    let bar_drv_derivation_or_fod_hash =
        bar_drv.derivation_or_fod_hash(|_| panic!("is FOD, should not lookup"));

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
    foo_env.insert("bar".to_string(), bar_output_path.to_owned().into());
    foo_env.insert("builder".to_string(), ":".into());
    foo_env.insert("name".to_string(), "foo".into());
    foo_env.insert("out".to_string(), "".into()); // will be calculated
    foo_env.insert("system".to_string(), ":".into());

    // asssemble foo outputs
    foo_drv.outputs.insert(
        "out".to_string(),
        Output {
            path: "".to_string(), // will be calculated
            hash_with_mode: None,
        },
    );

    // assemble foo input_derivations
    foo_drv.input_derivations.insert(
        bar_drv_path.to_absolute_path(),
        BTreeSet::from(["out".to_string()]),
    );

    // calculate foo output paths
    let foo_calc_result = foo_drv.calculate_output_paths(
        "foo",
        &foo_drv.derivation_or_fod_hash(|drv_path| {
            if drv_path != "/nix/store/0hm2f1psjpcwg8fijsmr4wwxrx59s092-bar.drv" {
                panic!("lookup called with unexpected drv_path: {}", drv_path);
            }
            bar_drv_derivation_or_fod_hash.clone()
        }),
    );
    assert!(foo_calc_result.is_ok());

    // ensure it matches our foo fixture
    let foo_data = read_file(&format!(
        "{}/{}.json",
        RESOURCES_PATHS, "4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv",
    ));
    let foo_drv_expected: Derivation = serde_json::from_slice(&foo_data).expect("must deserialize");
    assert_eq!(foo_drv_expected, foo_drv);

    assert_eq!(
        StorePath::from_str("4wvvbi4jwn0prsdxb7vs673qa5h9gr7x-foo.drv").expect("must succeed"),
        foo_drv
            .calculate_derivation_path("foo")
            .expect("must succeed")
    );
}

/// This constructs a Derivation using cp1252 encoding and ensures the
/// calculated derivation path matches the one Nix does calculate, as
/// well as the ATerm serialization.
/// We can't add this as a test_case to `output_paths`, as the JSON parser
/// refuses to parse our JSONs.
/// It looks like more recent versions of Nix also seem to not produce these
/// JSON files anymore, however, it still happily produces the .drv files in
/// the store.
#[test_case(
    "cp1252",
    vec![0xc5, 0xc4, 0xd6],
    "/nix/store/drr2mjp9fp9vvzsf5f9p0a80j33dxy7m-cp1252",
    "m1vfixn8iprlf0v9abmlrz7mjw1xj8kp-cp1252.drv";
    "cp1252"
)]
#[test_case(
    "latin1",
    vec![0xc5, 0xc4, 0xd6],
    "/nix/store/x1f6jfq9qgb6i8jrmpifkn9c64fg4hcm-latin1",
    "x6p0hg79i3wg0kkv7699935f7rrj9jf3-latin1.drv";
    "latin1"
)]
fn non_unicode(name: &str, chars: Vec<u8>, exp_output_path: &str, exp_derivation_path: &str) {
    // construct the Derivation
    let mut outputs: BTreeMap<String, Output> = BTreeMap::new();
    outputs.insert(
        "out".to_string(),
        Output {
            path: exp_output_path.to_string(),
            ..Default::default()
        },
    );

    let mut environment: BTreeMap<String, BString> = BTreeMap::new();
    environment.insert("builder".to_string(), ":".into());
    environment.insert("chars".to_string(), chars.into());
    environment.insert("name".to_string(), name.into());
    environment.insert("out".to_string(), exp_output_path.into());
    environment.insert("system".to_string(), ":".into());
    let derivation: Derivation = Derivation {
        builder: ":".to_string(),
        environment,
        outputs,
        system: ":".to_string(),
        ..Default::default()
    };

    // check the derivation_path matches what Nix calculated.
    let actual_drv_path = derivation.calculate_derivation_path(name).unwrap();
    assert_eq!(exp_derivation_path.to_string(), actual_drv_path.to_string());

    // Now wipe the output path info, and ensure we calculate the same output
    // path.
    {
        let mut derivation = derivation_with_trimmed_output_paths(&derivation);
        let calculated_derivation_or_fod_hash = derivation.derivation_or_fod_hash(|_| {
            panic!("No parents expected");
        });
        derivation
            .calculate_output_paths(name, &calculated_derivation_or_fod_hash)
            .unwrap();

        assert_eq!(
            exp_output_path.to_string(),
            derivation.outputs.get("out").unwrap().path,
            "expected calculated output path to match"
        );
    }

    // Construct the ATerm representation and compare with our fixture.
    {
        let data = read_file(&format!("{}/{}", RESOURCES_PATHS, exp_derivation_path));
        assert_eq!(
            data,
            BStr::new(&derivation.to_aterm_bytes()),
            "expected ATerm serialization to match",
        );
    }
}
