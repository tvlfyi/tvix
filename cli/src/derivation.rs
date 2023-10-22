//! Implements `builtins.derivation`, the core of what makes Nix build packages.
use nix_compat::derivation::{Derivation, Output};
use nix_compat::nixhash;
use std::cell::RefCell;
use std::collections::{btree_map, BTreeSet};
use std::rc::Rc;
use tvix_eval::builtin_macros::builtins;
use tvix_eval::generators::{self, emit_warning_kind, GenCo};
use tvix_eval::{
    AddContext, CatchableErrorKind, CoercionKind, ErrorKind, NixAttrs, NixList, Value, WarningKind,
};

use crate::errors::Error;
use crate::known_paths::{KnownPaths, PathKind, PathName};

// Constants used for strangely named fields in derivation inputs.
const STRUCTURED_ATTRS: &str = "__structuredAttrs";
const IGNORE_NULLS: &str = "__ignoreNulls";

/// Helper function for populating the `drv.outputs` field from a
/// manually specified set of outputs, instead of the default
/// `outputs`.
async fn populate_outputs(
    co: &GenCo,
    drv: &mut Derivation,
    outputs: NixList,
) -> Result<(), ErrorKind> {
    // Remove the original default `out` output.
    drv.outputs.clear();

    for output in outputs {
        let output_name = generators::request_force(co, output)
            .await
            .to_str()
            .context("determining output name")?;

        if drv
            .outputs
            .insert(output_name.as_str().into(), Default::default())
            .is_some()
        {
            return Err(Error::DuplicateOutput(output_name.as_str().into()).into());
        }
    }

    Ok(())
}

/// Populate the inputs of a derivation from the build references
/// found when scanning the derivation's parameters.
fn populate_inputs<I: IntoIterator<Item = PathName>>(
    drv: &mut Derivation,
    known_paths: &KnownPaths,
    references: I,
) {
    for reference in references.into_iter() {
        let reference = &known_paths[&reference];
        match &reference.kind {
            PathKind::Plain => {
                drv.input_sources.insert(reference.path.clone());
            }

            PathKind::Output { name, derivation } => {
                match drv.input_derivations.entry(derivation.clone()) {
                    btree_map::Entry::Vacant(entry) => {
                        entry.insert(BTreeSet::from([name.clone()]));
                    }

                    btree_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().insert(name.clone());
                    }
                }
            }

            PathKind::Derivation { output_names } => {
                match drv.input_derivations.entry(reference.path.clone()) {
                    btree_map::Entry::Vacant(entry) => {
                        entry.insert(output_names.clone());
                    }

                    btree_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().extend(output_names.clone().into_iter());
                    }
                }
            }
        }
    }
}

/// Populate the output configuration of a derivation based on the
/// parameters passed to the call, configuring a fixed-output derivation output
/// if necessary.
///
/// This function handles all possible combinations of the
/// parameters, including invalid ones.
///
/// Due to the support for SRI hashes, and how these are passed along to
/// builtins.derivation, outputHash and outputHashAlgo can have values which
/// need to be further modified before constructing the Derivation struct.
///
/// If outputHashAlgo is an SRI hash, outputHashAlgo must either be an empty
/// string, or the hash algorithm as specified in the (single) SRI (entry).
/// SRI strings with multiple hash algorithms are not supported.
///
/// In case an SRI string was used, the (single) fixed output is populated
/// with the hash algo name, and the hash digest is populated with the
/// (lowercase) hex encoding of the digest.
///
/// These values are only rewritten for the outputs, not what's passed to env.
fn handle_fixed_output(
    drv: &mut Derivation,
    hash_str: Option<String>,      // in nix: outputHash
    hash_algo_str: Option<String>, // in nix: outputHashAlgo
    hash_mode_str: Option<String>, // in nix: outputHashmode
) -> Result<(), ErrorKind> {
    // If outputHash is provided, ensure hash_algo_str is compatible.
    // If outputHash is not provided, do nothing.
    if let Some(hash_str) = hash_str {
        // treat an empty algo as None
        let hash_algo_str = match hash_algo_str {
            Some(s) if s.is_empty() => None,
            Some(s) => Some(s),
            None => None,
        };

        // construct a NixHash.
        let nixhash = nixhash::from_str(&hash_str, hash_algo_str.as_deref())
            .map_err(Error::InvalidOutputHash)?;

        // construct the fixed output.
        drv.outputs.insert(
            "out".to_string(),
            Output {
                path: "".to_string(),
                ca_hash: match hash_mode_str.as_deref() {
                    None | Some("flat") => Some(nixhash::CAHash::Flat(nixhash)),
                    Some("recursive") => Some(nixhash::CAHash::Nar(nixhash)),
                    Some(other) => return Err(Error::InvalidOutputHashMode(other.to_string()))?,
                },
            },
        );
    }
    Ok(())
}

/// Handles derivation parameters which are not just forwarded to
/// the environment. The return value indicates whether the
/// parameter should be included in the environment.
async fn handle_derivation_parameters(
    drv: &mut Derivation,
    co: &GenCo,
    name: &str,
    value: &Value,
    val_str: &str,
) -> Result<Result<bool, CatchableErrorKind>, ErrorKind> {
    match name {
        IGNORE_NULLS => return Ok(Ok(false)),

        // Command line arguments to the builder.
        "args" => {
            let args = value.to_list()?;
            for arg in args {
                match strong_coerce_to_string(co, arg).await? {
                    Err(cek) => return Ok(Err(cek)),
                    Ok(s) => drv.arguments.push(s),
                }
            }

            // The arguments do not appear in the environment.
            return Ok(Ok(false));
        }

        // Explicitly specified drv outputs (instead of default [ "out" ])
        "outputs" => {
            let outputs = value
                .to_list()
                .context("looking at the `outputs` parameter of the derivation")?;

            populate_outputs(co, drv, outputs).await?;
        }

        "builder" => {
            drv.builder = val_str.to_string();
        }

        "system" => {
            drv.system = val_str.to_string();
        }

        _ => {}
    }

    Ok(Ok(true))
}

async fn strong_coerce_to_string(
    co: &GenCo,
    val: Value,
) -> Result<Result<String, CatchableErrorKind>, ErrorKind> {
    let val = generators::request_force(co, val).await;
    match generators::request_string_coerce(co, val, CoercionKind::Strong).await {
        Err(cek) => Ok(Err(cek)),
        Ok(val_str) => Ok(Ok(val_str.as_str().to_string())),
    }
}

#[builtins(state = "Rc<RefCell<KnownPaths>>")]
mod derivation_builtins {
    use super::*;
    use nix_compat::store_path::hash_placeholder;
    use tvix_eval::generators::Gen;

    #[builtin("placeholder")]
    async fn builtin_placeholder(co: GenCo, input: Value) -> Result<Value, ErrorKind> {
        let placeholder = hash_placeholder(
            input
                .to_str()
                .context("looking at output name in builtins.placeholder")?
                .as_str(),
        );

        Ok(placeholder.into())
    }

    /// Strictly construct a Nix derivation from the supplied arguments.
    ///
    /// This is considered an internal function, users usually want to
    /// use the higher-level `builtins.derivation` instead.
    #[builtin("derivationStrict")]
    async fn builtin_derivation_strict(
        state: Rc<RefCell<KnownPaths>>,
        co: GenCo,
        input: Value,
    ) -> Result<Value, ErrorKind> {
        let input = input.to_attrs()?;
        let name = generators::request_force(&co, input.select_required("name")?.clone())
            .await
            .to_str()
            .context("determining derivation name")?;

        if name.is_empty() {
            return Err(ErrorKind::Abort("derivation has empty name".to_string()));
        }

        // Check whether attributes should be passed as a JSON file.
        // TODO: the JSON serialisation has to happen here.
        if let Some(sa) = input.select(STRUCTURED_ATTRS) {
            if generators::request_force(&co, sa.clone()).await.as_bool()? {
                return Err(ErrorKind::NotImplemented(STRUCTURED_ATTRS));
            }
        }

        // Check whether null attributes should be ignored or passed through.
        let ignore_nulls = match input.select(IGNORE_NULLS) {
            Some(b) => generators::request_force(&co, b.clone()).await.as_bool()?,
            None => false,
        };

        let mut drv = Derivation::default();
        drv.outputs.insert("out".to_string(), Default::default());

        async fn select_string(
            co: &GenCo,
            attrs: &NixAttrs,
            key: &str,
        ) -> Result<Result<Option<String>, CatchableErrorKind>, ErrorKind> {
            if let Some(attr) = attrs.select(key) {
                match strong_coerce_to_string(co, attr.clone()).await? {
                    Err(cek) => return Ok(Err(cek)),
                    Ok(str) => return Ok(Ok(Some(str))),
                }
            }

            Ok(Ok(None))
        }

        for (name, value) in input.clone().into_iter_sorted() {
            let value = generators::request_force(&co, value).await;
            if ignore_nulls && matches!(value, Value::Null) {
                continue;
            }

            match strong_coerce_to_string(&co, value.clone()).await? {
                Err(cek) => return Ok(Value::Catchable(cek)),
                Ok(val_str) => {
                    // handle_derivation_parameters tells us whether the
                    // argument should be added to the environment; continue
                    // to the next one otherwise
                    match handle_derivation_parameters(
                        &mut drv,
                        &co,
                        name.as_str(),
                        &value,
                        &val_str,
                    )
                    .await?
                    {
                        Err(cek) => return Ok(Value::Catchable(cek)),
                        Ok(false) => continue,
                        _ => (),
                    }

                    // Most of these are also added to the builder's environment in "raw" form.
                    if drv
                        .environment
                        .insert(name.as_str().to_string(), val_str.into())
                        .is_some()
                    {
                        return Err(Error::DuplicateEnvVar(name.as_str().to_string()).into());
                    }
                }
            }
        }

        // Configure fixed-output derivations if required.
        {
            let output_hash = match select_string(&co, &input, "outputHash")
                .await
                .context("evaluating the `outputHash` parameter")?
            {
                Err(cek) => return Ok(Value::Catchable(cek)),
                Ok(s) => s,
            };
            let output_hash_algo = match select_string(&co, &input, "outputHashAlgo")
                .await
                .context("evaluating the `outputHashAlgo` parameter")?
            {
                Err(cek) => return Ok(Value::Catchable(cek)),
                Ok(s) => s,
            };
            let output_hash_mode = match select_string(&co, &input, "outputHashMode")
                .await
                .context("evaluating the `outputHashMode` parameter")?
            {
                Err(cek) => return Ok(Value::Catchable(cek)),
                Ok(s) => s,
            };
            handle_fixed_output(&mut drv, output_hash, output_hash_algo, output_hash_mode)?;
        }

        // Scan references in relevant attributes to detect any build-references.
        let references = {
            let state = state.borrow();
            if state.is_empty() {
                // skip reference scanning, create an empty result
                Default::default()
            } else {
                let mut refscan = state.reference_scanner();
                drv.arguments.iter().for_each(|s| refscan.scan(s));
                drv.environment.values().for_each(|s| refscan.scan(s));
                refscan.scan(&drv.builder);
                refscan.finalise()
            }
        };

        // Each output name needs to exist in the environment, at this
        // point initialised as an empty string because that is the
        // way of Golang ;)
        for output in drv.outputs.keys() {
            if drv
                .environment
                .insert(output.to_string(), String::new().into())
                .is_some()
            {
                emit_warning_kind(&co, WarningKind::ShadowedOutput(output.to_string())).await;
            }
        }

        let mut known_paths = state.borrow_mut();
        populate_inputs(&mut drv, &known_paths, references);

        // At this point, derivation fields are fully populated from
        // eval data structures.
        drv.validate(false).map_err(Error::InvalidDerivation)?;

        // Calculate the derivation_or_fod_hash for the current derivation.
        // This one is still intermediate (so not added to known_paths)
        let derivation_or_fod_hash_tmp =
            drv.derivation_or_fod_hash(|drv| known_paths.get_hash_derivation_modulo(drv));

        // Mutate the Derivation struct and set output paths
        drv.calculate_output_paths(&name, &derivation_or_fod_hash_tmp)
            .map_err(Error::InvalidDerivation)?;

        let derivation_path = drv
            .calculate_derivation_path(&name)
            .map_err(Error::InvalidDerivation)?;

        // recompute the hash derivation modulo and add to known_paths
        let derivation_or_fod_hash_final =
            drv.derivation_or_fod_hash(|drv| known_paths.get_hash_derivation_modulo(drv));

        known_paths.add_hash_derivation_modulo(
            derivation_path.to_absolute_path(),
            &derivation_or_fod_hash_final,
        );

        // mark all the new paths as known
        let output_names: Vec<String> = drv.outputs.keys().map(Clone::clone).collect();
        known_paths.drv(derivation_path.to_absolute_path(), &output_names);

        for (output_name, output) in &drv.outputs {
            known_paths.output(
                &output.path,
                output_name,
                derivation_path.to_absolute_path(),
            );
        }

        let mut new_attrs: Vec<(String, String)> = drv
            .outputs
            .into_iter()
            .map(|(name, output)| (name, output.path))
            .collect();

        new_attrs.push(("drvPath".to_string(), derivation_path.to_absolute_path()));

        Ok(Value::Attrs(Box::new(NixAttrs::from_iter(
            new_attrs.into_iter(),
        ))))
    }

    #[builtin("toFile")]
    async fn builtin_to_file(
        state: Rc<RefCell<KnownPaths>>,
        co: GenCo,
        name: Value,
        content: Value,
    ) -> Result<Value, ErrorKind> {
        let name = name
            .to_str()
            .context("evaluating the `name` parameter of builtins.toFile")?;
        let content = content
            .to_str()
            .context("evaluating the `content` parameter of builtins.toFile")?;

        let mut refscan = state.borrow().reference_scanner();
        refscan.scan(content.as_str());
        let refs = {
            let paths = state.borrow();
            refscan
                .finalise()
                .into_iter()
                .map(|path| paths[&path].path.to_string())
                .collect::<Vec<_>>()
        };

        // TODO: fail on derivation references (only "plain" is allowed here)

        let path = nix_compat::store_path::build_text_path(name.as_str(), content.as_str(), refs)
            .map_err(|_e| {
                nix_compat::derivation::DerivationError::InvalidOutputName(
                    name.as_str().to_string(),
                )
            })
            .map_err(Error::InvalidDerivation)?
            .to_absolute_path();

        state.borrow_mut().plain(&path);

        // TODO: actually persist the file in the store at that path ...

        Ok(Value::String(path.into()))
    }
}

pub use derivation_builtins::builtins as derivation_builtins;

#[cfg(test)]
mod tests {
    use crate::known_paths::KnownPaths;
    use nix_compat::store_path::hash_placeholder;
    use std::{cell::RefCell, rc::Rc};
    use test_case::test_case;
    use tvix_eval::EvaluationResult;

    /// evaluates a given nix expression and returns the result.
    /// Takes care of setting up the evaluator so it knows about the
    // `derivation` builtin.
    fn eval(str: &str) -> EvaluationResult {
        let mut eval = tvix_eval::Evaluation::new_impure(str, None);

        let known_paths: Rc<RefCell<KnownPaths>> = Default::default();

        eval.builtins
            .extend(crate::derivation::derivation_builtins(known_paths));

        // Add the actual `builtins.derivation` from compiled Nix code
        eval.src_builtins
            .push(("derivation", include_str!("derivation.nix")));

        // run the evaluation itself.
        eval.evaluate()
    }

    #[test]
    fn derivation() {
        let result = eval(
            r#"(derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux";}).outPath"#,
        );

        assert!(result.errors.is_empty(), "expect evaluation to succeed");
        let value = result.value.expect("must be some");

        match value {
            tvix_eval::Value::String(s) => {
                assert_eq!(
                    "/nix/store/xpcvxsx5sw4rbq666blz6sxqlmsqphmr-foo",
                    s.as_str()
                );
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }

    /// a derivation with an empty name is an error.
    #[test]
    fn derivation_empty_name_fail() {
        let result = eval(
            r#"(derivation { name = ""; builder = "/bin/sh"; system = "x86_64-linux";}).outPath"#,
        );

        assert!(!result.errors.is_empty(), "expect evaluation to fail");
    }

    /// construct some calls to builtins.derivation and compare produced output
    /// paths.
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#, "/nix/store/17wgs52s7kcamcyin4ja58njkf91ipq8-foo"; "r:sha256")]
    #[test_case(r#"(builtins.derivation { name = "foo2"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#, "/nix/store/gi0p8vd635vpk1nq029cz3aa3jkhar5k-foo2"; "r:sha256 other name")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha1"; outputHash = "sha1-VUCRC+16gU5lcrLYHlPSUyx0Y/Q="; }).outPath"#, "/nix/store/p5sammmhpa84ama7ymkbgwwzrilva24x-foo"; "r:sha1")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "md5"; outputHash = "md5-07BzhNET7exJ6qYjitX/AA=="; }).outPath"#, "/nix/store/gmmxgpy1jrzs86r5y05wy6wiy2m15xgi-foo"; "r:md5")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha512"; outputHash = "sha512-DPkYCnZKuoY6Z7bXLwkYvBMcZ3JkLLLc5aNPCnAvlHDdwr8SXBIZixmVwjPDS0r9NGxUojNMNQqUilG26LTmtg=="; }).outPath"#, "/nix/store/lfi2bfyyap88y45mfdwi4j99gkaxaj19-foo"; "r:sha512")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "4374173a8cbe88de152b609f96f46e958bcf65762017474eec5a05ec2bd61530"; }).outPath"#, "/nix/store/17wgs52s7kcamcyin4ja58njkf91ipq8-foo"; "r:sha256 base16")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "0c0msqmyq1asxi74f5r0frjwz2wmdvs9d7v05caxx25yihx1fx23"; }).outPath"#, "/nix/store/17wgs52s7kcamcyin4ja58njkf91ipq8-foo"; "r:sha256 nixbase32")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#, "/nix/store/17wgs52s7kcamcyin4ja58njkf91ipq8-foo"; "r:sha256 base64")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "sha256-fgIr3TyFGDAXP5+qoAaiMKDg/a1MlT6Fv/S/DaA24S8="; }).outPath"#, "/nix/store/xm1l9dx4zgycv9qdhcqqvji1z88z534b-foo"; "r:sha256 base64 nopad")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "flat"; outputHashAlgo = "sha256"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#, "/nix/store/q4pkwkxdib797fhk22p0k3g1q32jmxvf-foo"; "sha256")]
    #[test_case(r#"(builtins.derivation { name = "foo2"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "flat"; outputHashAlgo = "sha256"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#, "/nix/store/znw17xlmx9r6gw8izjkqxkl6s28sza4l-foo2"; "sha256 other name")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "flat"; outputHashAlgo = "sha1"; outputHash = "sha1-VUCRC+16gU5lcrLYHlPSUyx0Y/Q="; }).outPath"#, "/nix/store/zgpnjjmga53d8srp8chh3m9fn7nnbdv6-foo"; "sha1")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "flat"; outputHashAlgo = "md5"; outputHash = "md5-07BzhNET7exJ6qYjitX/AA=="; }).outPath"#, "/nix/store/jfhcwnq1852ccy9ad9nakybp2wadngnd-foo"; "md5")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "flat"; outputHashAlgo = "sha512"; outputHash = "sha512-DPkYCnZKuoY6Z7bXLwkYvBMcZ3JkLLLc5aNPCnAvlHDdwr8SXBIZixmVwjPDS0r9NGxUojNMNQqUilG26LTmtg=="; }).outPath"#, "/nix/store/as736rr116ian9qzg457f96j52ki8bm3-foo"; "sha512")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#, "/nix/store/17wgs52s7kcamcyin4ja58njkf91ipq8-foo"; "r:sha256 outputHashAlgo omitted")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#, "/nix/store/q4pkwkxdib797fhk22p0k3g1q32jmxvf-foo"; "r:sha256 outputHashAlgo and outputHashMode omitted")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; }).outPath"#, "/nix/store/xpcvxsx5sw4rbq666blz6sxqlmsqphmr-foo"; "outputHash* omitted")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; outputs = ["foo" "bar"]; system = "x86_64-linux"; }).outPath"#, "/nix/store/hkwdinvz2jpzgnjy9lv34d2zxvclj4s3-foo-foo"; "multiple outputs")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; args = ["--foo" "42" "--bar"]; system = "x86_64-linux"; }).outPath"#, "/nix/store/365gi78n2z7vwc1bvgb98k0a9cqfp6as-foo"; "args")]
    #[test_case(r#"
                   let
                     bar = builtins.derivation {
                       name = "bar";
                       builder = ":";
                       system = ":";
                       outputHash = "08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba";
                       outputHashAlgo = "sha256";
                       outputHashMode = "recursive";
                     };
                   in
                   (builtins.derivation {
                     name = "foo";
                     builder = ":";
                     system = ":";
                     inherit bar;
                   }).outPath
        "#, "/nix/store/5vyvcwah9l9kf07d52rcgdk70g2f4y13-foo"; "full")]
    fn test_outpath(code: &str, expected_path: &str) {
        let value = eval(code).value.expect("must succeed");

        match value {
            tvix_eval::Value::String(s) => {
                assert_eq!(expected_path, s.as_str());
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }

    /// construct some calls to builtins.derivation that should be rejected
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "sha256-00"; }).outPath"#; "invalid outputhash")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha1"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath"#; "sha1 and sha256")]
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; outputs = ["foo" "foo"]; system = "x86_64-linux"; }).outPath"#; "duplicate output names")]
    fn test_outpath_invalid(code: &str) {
        let resp = eval(code);
        assert!(resp.value.is_none(), "Value should be None");
        assert!(
            !resp.errors.is_empty(),
            "There should have been some errors"
        );
    }

    #[test]
    fn builtins_placeholder_hashes() {
        assert_eq!(
            hash_placeholder("out").as_str(),
            "/1rz4g4znpzjwh1xymhjpm42vipw92pr73vdgl6xs1hycac8kf2n9"
        );

        assert_eq!(
            hash_placeholder("").as_str(),
            "/171rf4jhx57xqz3p7swniwkig249cif71pa08p80mgaf0mqz5bmr"
        );
    }
}
