//! Contains builtins that deal with the store or builder.

use std::rc::Rc;

use crate::tvix_store_io::TvixStoreIO;

mod derivation;
mod errors;
mod fetchers;
mod import;

pub use errors::DerivationError;

/// Adds derivation-related builtins to the passed [tvix_eval::Evaluation].
///
/// These are `derivation` and `derivationStrict`.
///
/// As they need to interact with `known_paths`, we also need to pass in
/// `known_paths`.
pub fn add_derivation_builtins<IO>(eval: &mut tvix_eval::Evaluation<IO>, io: Rc<TvixStoreIO>) {
    eval.builtins
        .extend(derivation::derivation_builtins::builtins(Rc::clone(&io)));

    // Add the actual `builtins.derivation` from compiled Nix code
    eval.src_builtins
        .push(("derivation", include_str!("derivation.nix")));
}

/// Adds fetcher builtins to the passed [tvix_eval::Evaluation]:
///
/// * `fetchurl`
/// * `fetchTarball`
/// * `fetchGit`
pub fn add_fetcher_builtins<IO>(eval: &mut tvix_eval::Evaluation<IO>, io: Rc<TvixStoreIO>) {
    eval.builtins
        .extend(fetchers::fetcher_builtins::builtins(Rc::clone(&io)));
}

/// Adds import-related builtins to the passed [tvix_eval::Evaluation].
///
/// These are `filterSource` and `path`
///
/// As they need to interact with the store implementation, we pass [`TvixStoreIO`].
pub fn add_import_builtins<IO>(eval: &mut tvix_eval::Evaluation<IO>, io: Rc<TvixStoreIO>) {
    eval.builtins.extend(import::import_builtins(io));

    // TODO(raitobezarius): evaluate expressing filterSource as Nix code using path (b/372)
}

#[cfg(test)]
mod tests {
    use std::{fs, rc::Rc, sync::Arc};

    use crate::tvix_store_io::TvixStoreIO;

    use super::{add_derivation_builtins, add_fetcher_builtins, add_import_builtins};
    use nix_compat::store_path::hash_placeholder;
    use tempfile::TempDir;
    use test_case::test_case;
    use tvix_build::buildservice::DummyBuildService;
    use tvix_eval::{EvalIO, EvaluationResult};
    use tvix_store::utils::construct_services;

    /// evaluates a given nix expression and returns the result.
    /// Takes care of setting up the evaluator so it knows about the
    // `derivation` builtin.
    fn eval(str: &str) -> EvaluationResult {
        // We assemble a complete store in memory.
        let runtime = tokio::runtime::Runtime::new().expect("Failed to build a Tokio runtime");
        let (blob_service, directory_service, path_info_service) = runtime
            .block_on(async { construct_services("memory://", "memory://", "memory://").await })
            .expect("Failed to construct store services in memory");

        let io = Rc::new(TvixStoreIO::new(
            blob_service,
            directory_service,
            path_info_service.into(),
            Arc::<DummyBuildService>::default(),
            runtime.handle().clone(),
        ));

        let mut eval = tvix_eval::Evaluation::new(io.clone() as Rc<dyn EvalIO>, false);

        add_derivation_builtins(&mut eval, Rc::clone(&io));
        add_fetcher_builtins(&mut eval, Rc::clone(&io));
        add_import_builtins(&mut eval, io);

        // run the evaluation itself.
        eval.evaluate(str, None)
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
                assert_eq!(*s, "/nix/store/xpcvxsx5sw4rbq666blz6sxqlmsqphmr-foo",);
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
    #[test_case(r#"(builtins.derivation { "name" = "foo"; passAsFile = ["bar"]; bar = "baz"; system = ":"; builder = ":";}).outPath"#, "/nix/store/25gf0r1ikgmh4vchrn8qlc4fnqlsa5a1-foo"; "passAsFile")]
    // __ignoreNulls = true, but nothing set to null
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = true; }).drvPath"#, "/nix/store/xa96w6d7fxrlkk60z1fmx2ffp2wzmbqx-foo.drv"; "ignoreNulls no arg drvPath")]
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = true; }).outPath"#, "/nix/store/pk2agn9za8r9bxsflgh1y7fyyrmwcqkn-foo"; "ignoreNulls no arg outPath")]
    // __ignoreNulls = true, with a null arg, same paths
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = true; ignoreme = null; }).drvPath"#, "/nix/store/xa96w6d7fxrlkk60z1fmx2ffp2wzmbqx-foo.drv"; "ignoreNulls drvPath")]
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = true; ignoreme = null; }).outPath"#, "/nix/store/pk2agn9za8r9bxsflgh1y7fyyrmwcqkn-foo"; "ignoreNulls outPath")]
    // __ignoreNulls = false
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = false; }).drvPath"#, "/nix/store/xa96w6d7fxrlkk60z1fmx2ffp2wzmbqx-foo.drv"; "ignoreNulls false no arg drvPath")]
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = false; }).outPath"#, "/nix/store/pk2agn9za8r9bxsflgh1y7fyyrmwcqkn-foo"; "ignoreNulls false no arg arg outPath")]
    // __ignoreNulls = false, with a null arg
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = false; foo = null; }).drvPath"#, "/nix/store/xwkwbajfiyhdqmksrbzm0s4g4ib8d4ms-foo.drv"; "ignoreNulls false arg drvPath")]
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = false; foo = null; }).outPath"#, "/nix/store/2n2jqm6l7r2ahi19m58pl896ipx9cyx6-foo"; "ignoreNulls false arg arg outPath")]
    // structured attrs set to false will render an empty string inside env
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __structuredAttrs = false; foo = "bar"; }).drvPath"#, "/nix/store/qs39krwr2lsw6ac910vqx4pnk6m63333-foo.drv"; "structuredAttrs-false-drvPath")]
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __structuredAttrs = false; foo = "bar"; }).outPath"#, "/nix/store/9yy3764rdip3fbm8ckaw4j9y7vh4d231-foo"; "structuredAttrs-false-outPath")]
    // simple structured attrs
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __structuredAttrs = true; foo = "bar"; }).drvPath"#, "/nix/store/k6rlb4k10cb9iay283037ml1nv3xma2f-foo.drv"; "structuredAttrs-simple-drvPath")]
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __structuredAttrs = true; foo = "bar"; }).outPath"#, "/nix/store/6lmv3hyha1g4cb426iwjyifd7nrdv1xn-foo"; "structuredAttrs-simple-outPath")]
    // structured attrs with outputsCheck
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __structuredAttrs = true; foo = "bar"; outputChecks = {out = {maxClosureSize = 256 * 1024 * 1024; disallowedRequisites = [ "dev" ];};}; }).drvPath"#, "/nix/store/fx9qzpchh5wchchhy39bwsml978d6wp1-foo.drv"; "structuredAttrs-outputChecks-drvPath")]
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __structuredAttrs = true; foo = "bar"; outputChecks = {out = {maxClosureSize = 256 * 1024 * 1024; disallowedRequisites = [ "dev" ];};}; }).outPath"#, "/nix/store/pcywah1nwym69rzqdvpp03sphfjgyw1l-foo"; "structuredAttrs-outputChecks-outPath")]
    // structured attrs and __ignoreNulls. ignoreNulls is inactive (so foo ends up in __json, yet __ignoreNulls itself is not present.
    #[test_case(r#"(builtins.derivation { name = "foo"; system = ":"; builder = ":"; __ignoreNulls = false; foo = null; __structuredAttrs = true; }).drvPath"#, "/nix/store/rldskjdcwa3p7x5bqy3r217va1jsbjsc-foo.drv"; "structuredAttrs-and-ignore-nulls-drvPath")]

    fn test_outpath(code: &str, expected_path: &str) {
        let value = eval(code).value.expect("must succeed");

        match value {
            tvix_eval::Value::String(s) => {
                assert_eq!(*s, expected_path);
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

    /// Construct two FODs with the same name, and same known output (but
    /// slightly different recipe), ensure they have the same output hash.
    #[test]
    fn test_fod_outpath() {
        let code = r#"
          (builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath ==
          (builtins.derivation { name = "foo"; builder = "/bin/aa"; system = "x86_64-linux"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath
        "#;

        let value = eval(code).value.expect("must succeed");
        match value {
            tvix_eval::Value::Bool(v) => {
                assert!(v);
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }

    /// Construct two FODs with the same name, and same known output (but
    /// slightly different recipe), ensure they have the same output hash.
    #[test]
    fn test_fod_outpath_different_name() {
        let code = r#"
          (builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath ==
          (builtins.derivation { name = "foo"; builder = "/bin/aa"; system = "x86_64-linux"; outputHash = "sha256-Q3QXOoy+iN4VK2CflvRulYvPZXYgF0dO7FoF7CvWFTA="; }).outPath
        "#;

        let value = eval(code).value.expect("must succeed");
        match value {
            tvix_eval::Value::Bool(v) => {
                assert!(v);
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }

    /// Construct two derivations with the same parameters except one of them lost a context string
    /// for a dependency, causing the loss of an element in the `inputDrvs` derivation. Therefore,
    /// making `outPath` different.
    #[test]
    fn test_unsafe_discard_string_context() {
        let code = r#"
        let
            dep = builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; };
        in
          (builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; env = "${dep}"; }).outPath !=
          (builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; env = "${builtins.unsafeDiscardStringContext dep}"; }).outPath
        "#;

        let value = eval(code).value.expect("must succeed");
        match value {
            tvix_eval::Value::Bool(v) => {
                assert!(v);
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }

    /// Construct an attribute set that coerces to a derivation and verify that the return type is
    /// a string.
    #[test]
    fn test_unsafe_discard_string_context_of_coercible() {
        let code = r#"
        let
            dep = builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; };
            attr = { __toString = _: dep; };
        in
            builtins.typeOf (builtins.unsafeDiscardStringContext attr) == "string"
        "#;

        let value = eval(code).value.expect("must succeed");
        match value {
            tvix_eval::Value::Bool(v) => {
                assert!(v);
            }
            _ => panic!("unexpected value type: {:?}", value),
        }
    }

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
                     args = [ "${bar}" ];
                     system = ":";
                   }).drvPath
        "#, "/nix/store/50yl2gmmljyl0lzyrp1mcyhn53vhjhkd-foo.drv"; "input in `args`")]
    fn test_inputs_derivation_from_context(code: &str, expected_drvpath: &str) {
        let eval_result = eval(code);

        let value = eval_result.value.expect("must succeed");

        match value {
            tvix_eval::Value::String(s) => {
                assert_eq!(*s, expected_drvpath);
            }

            _ => panic!("unexpected value type: {:?}", value),
        };
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

    /// constructs calls to builtins.derivation that should succeed, but produce warnings
    #[test_case(r#"(builtins.derivation { name = "foo"; builder = "/bin/sh"; system = "x86_64-linux"; outputHashMode = "recursive"; outputHashAlgo = "sha256"; outputHash = "sha256-fgIr3TyFGDAXP5+qoAaiMKDg/a1MlT6Fv/S/DaA24S8===="; }).outPath"#, "/nix/store/xm1l9dx4zgycv9qdhcqqvji1z88z534b-foo"; "r:sha256 wrong padding")]
    fn builtins_derivation_hash_wrong_padding_warn(code: &str, expected_path: &str) {
        let eval_result = eval(code);

        let value = eval_result.value.expect("must succeed");

        match value {
            tvix_eval::Value::String(s) => {
                assert_eq!(*s, expected_path);
            }
            _ => panic!("unexpected value type: {:?}", value),
        }

        assert!(
            !eval_result.warnings.is_empty(),
            "warnings should not be empty"
        );
    }

    /// Invokes `builtins.filterSource` on various carefully-crated subdirs, and
    /// ensures the resulting store paths matches what Nix produces.
    /// @fixtures is replaced to the fixtures directory.
    #[cfg(target_family = "unix")]
    #[test_case(r#"(builtins.filterSource (p: t: true) @fixtures)"#, "/nix/store/bqh6kd0x3vps2rzagzpl7qmbbgnx19cp-import_fixtures"; "complicated directory: filter nothing")]
    #[test_case(r#"(builtins.filterSource (p: t: false) @fixtures)"#, "/nix/store/giq6czz24lpjg97xxcxk6rg950lcpib1-import_fixtures"; "complicated directory: filter everything")]
    #[test_case(r#"(builtins.filterSource (p: t: t != "directory") @fixtures/a_dir)"#, "/nix/store/8vbqaxapywkvv1hacdja3pi075r14d43-a_dir"; "simple directory with one file: filter directories")]
    #[test_case(r#"(builtins.filterSource (p: t: t != "regular") @fixtures/a_dir)"#, "/nix/store/zphlqc93s2iq4xm393l06hzf8hp85r4z-a_dir"; "simple directory with one file: filter files")]
    #[test_case(r#"(builtins.filterSource (p: t: t != "symlink") @fixtures/a_dir)"#, "/nix/store/8vbqaxapywkvv1hacdja3pi075r14d43-a_dir"; "simple directory with one file: filter symlinks")]
    #[test_case(r#"(builtins.filterSource (p: t: true) @fixtures/a_dir)"#, "/nix/store/8vbqaxapywkvv1hacdja3pi075r14d43-a_dir"; "simple directory with one file: filter nothing")]
    #[test_case(r#"(builtins.filterSource (p: t: false) @fixtures/a_dir)"#, "/nix/store/zphlqc93s2iq4xm393l06hzf8hp85r4z-a_dir"; "simple directory with one file: filter everything")]
    #[test_case(r#"builtins.filterSource (p: t: t != "directory") @fixtures/b_dir"#, "/nix/store/xzsfzdgrxg93icaamjm8zq1jq6xvf2fz-b_dir"; "simple directory with one directory: filter directories")]
    #[test_case(r#"builtins.filterSource (p: t: t != "regular") @fixtures/b_dir"#, "/nix/store/8rjx64mm7173xp60rahv7cl3ixfkv3rf-b_dir"; "simple directory with one directory: filter files")]
    #[test_case(r#"builtins.filterSource (p: t: t != "symlink") @fixtures/b_dir"#, "/nix/store/8rjx64mm7173xp60rahv7cl3ixfkv3rf-b_dir"; "simple directory with one directory: filter symlinks")]
    #[test_case(r#"builtins.filterSource (p: t: true) @fixtures/b_dir"#, "/nix/store/8rjx64mm7173xp60rahv7cl3ixfkv3rf-b_dir"; "simple directory with one directory: filter nothing")]
    #[test_case(r#"builtins.filterSource (p: t: false) @fixtures/b_dir"#, "/nix/store/xzsfzdgrxg93icaamjm8zq1jq6xvf2fz-b_dir"; "simple directory with one directory: filter everything")]
    #[test_case(r#"builtins.filterSource (p: t: t != "directory") @fixtures/c_dir"#, "/nix/store/riigfmmzzrq65zqiffcjk5sbqr9c9h09-c_dir"; "simple directory with one symlink to a file: filter directory")]
    #[test_case(r#"builtins.filterSource (p: t: t != "regular") @fixtures/c_dir"#, "/nix/store/riigfmmzzrq65zqiffcjk5sbqr9c9h09-c_dir"; "simple directory with one symlink to a file: filter files")]
    #[test_case(r#"builtins.filterSource (p: t: t != "symlink") @fixtures/c_dir"#, "/nix/store/y5g1fz04vzjvf422q92qmv532axj5q26-c_dir"; "simple directory with one symlink to a file: filter symlinks")]
    #[test_case(r#"builtins.filterSource (p: t: true) @fixtures/c_dir"#, "/nix/store/riigfmmzzrq65zqiffcjk5sbqr9c9h09-c_dir"; "simple directory with one symlink to a file: filter nothing")]
    #[test_case(r#"builtins.filterSource (p: t: false) @fixtures/c_dir"#, "/nix/store/y5g1fz04vzjvf422q92qmv532axj5q26-c_dir"; "simple directory with one symlink to a file: filter everything")]
    #[test_case(r#"builtins.filterSource (p: t: t != "directory") @fixtures/d_dir"#, "/nix/store/f2d1aixwiqy4lbzrd040ala2s4m2z199-d_dir"; "simple directory with dangling symlink: filter directory")]
    #[test_case(r#"builtins.filterSource (p: t: t != "regular") @fixtures/d_dir"#, "/nix/store/f2d1aixwiqy4lbzrd040ala2s4m2z199-d_dir"; "simple directory with dangling symlink: filter file")]
    #[test_case(r#"builtins.filterSource (p: t: t != "symlink") @fixtures/d_dir"#, "/nix/store/7l371xax8kknhpska4wrmyll1mzlhzvl-d_dir"; "simple directory with dangling symlink: filter symlinks")]
    #[test_case(r#"builtins.filterSource (p: t: true) @fixtures/d_dir"#, "/nix/store/f2d1aixwiqy4lbzrd040ala2s4m2z199-d_dir"; "simple directory with dangling symlink: filter nothing")]
    #[test_case(r#"builtins.filterSource (p: t: false) @fixtures/d_dir"#, "/nix/store/7l371xax8kknhpska4wrmyll1mzlhzvl-d_dir"; "simple directory with dangling symlink: filter everything")]
    #[test_case(r#"builtins.filterSource (p: t: t != "directory") @fixtures/symlink_to_a_dir"#, "/nix/store/apmdprm8fwl2zrjpbyfcd99zrnhvf47q-symlink_to_a_dir"; "simple symlinked directory with one file: filter directories")]
    #[test_case(r#"builtins.filterSource (p: t: t != "regular") @fixtures/symlink_to_a_dir"#, "/nix/store/apmdprm8fwl2zrjpbyfcd99zrnhvf47q-symlink_to_a_dir"; "simple symlinked directory with one file: filter file")]
    #[test_case(r#"builtins.filterSource (p: t: t != "symlink") @fixtures/symlink_to_a_dir"#, "/nix/store/apmdprm8fwl2zrjpbyfcd99zrnhvf47q-symlink_to_a_dir"; "simple symlinked directory with one file: filter symlinks")]
    #[test_case(r#"builtins.filterSource (p: t: true) @fixtures/symlink_to_a_dir"#, "/nix/store/apmdprm8fwl2zrjpbyfcd99zrnhvf47q-symlink_to_a_dir"; "simple symlinked directory with one file: filter nothing")]
    #[test_case(r#"builtins.filterSource (p: t: false) @fixtures/symlink_to_a_dir"#, "/nix/store/apmdprm8fwl2zrjpbyfcd99zrnhvf47q-symlink_to_a_dir"; "simple symlinked directory with one file: filter everything")]
    fn builtins_filter_source_succeed(code: &str, expected_outpath: &str) {
        // populate the fixtures dir
        let temp = TempDir::new().expect("create temporary directory");
        let p = temp.path().join("import_fixtures");

        // create the fixtures directory.
        // We produce them at runtime rather than shipping it inside the source
        // tree, as git can't model certain things - like directories without any
        // items.
        {
            fs::create_dir(&p).expect("creating import_fixtures");

            // `/a_dir` contains an empty `a_file` file
            fs::create_dir(p.join("a_dir")).expect("creating /a_dir");
            fs::write(p.join("a_dir").join("a_file"), "").expect("creating /a_dir/a_file");

            // `/a_file` is an empty file
            fs::write(p.join("a_file"), "").expect("creating /a_file");

            // `/b_dir` contains an empty "a_dir" directory
            fs::create_dir_all(p.join("b_dir").join("a_dir")).expect("creating /b_dir/a_dir");

            // `/c_dir` contains a `symlink_to_a_file` symlink, pointing to `../a_dir/a_file`.
            fs::create_dir(p.join("c_dir")).expect("creating /c_dir");
            std::os::unix::fs::symlink(
                "../a_dir/a_file",
                p.join("c_dir").join("symlink_to_a_file"),
            )
            .expect("creating /c_dir/symlink_to_a_file");

            // `/d_dir` contains a `dangling_symlink`, pointing to `a_dir/a_file`,
            // which does not exist.
            fs::create_dir(p.join("d_dir")).expect("creating /d_dir");
            std::os::unix::fs::symlink("a_dir/a_file", p.join("d_dir").join("dangling_symlink"))
                .expect("creating /d_dir/dangling_symlink");

            // `/symlink_to_a_dir` is a symlink to `a_dir`, which exists.
            std::os::unix::fs::symlink("a_dir", p.join("symlink_to_a_dir"))
                .expect("creating /symlink_to_a_dir");
        }

        // replace @fixtures with the temporary path containing the fixtures
        let code_replaced = code.replace("@fixtures", &p.to_string_lossy());

        let eval_result = eval(&code_replaced);

        let value = eval_result.value.expect("must succeed");

        match value {
            tvix_eval::Value::String(s) => {
                assert_eq!(expected_outpath, s.as_bstr());
            }
            _ => panic!("unexpected value type: {:?}", value),
        }

        assert!(eval_result.errors.is_empty(), "errors should be empty");
    }

    // All tests filter out some unsupported (not representable in castore) nodes, confirming
    // invalid, but filtered-out nodes don't prevent ingestion of a path.
    #[cfg(target_family = "unix")]
    // There is a set of invalid filetypes.
    // We write a filter function for most subsets, excluding one that filters all of them.
    // We expect these cases to make the evaluation fail as there are still invalid files present
    // after the filtering.
    #[test_case(r#"(builtins.filterSource (p: t: t == "unknown") @fixtures)"#, false; "complicated directory: filter unsupported types")]
    #[test_case(r#"(builtins.filterSource (p: t: (builtins.baseNameOf p) != "a_charnode") @fixtures)"#, false; "complicated directory: filter character device nodes")]
    #[test_case(r#"(builtins.filterSource (p: t: (builtins.baseNameOf p) != "a_socket") @fixtures)"#, false; "complicated directory: filter sockets")]
    #[test_case(r#"(builtins.filterSource (p: t: (builtins.baseNameOf p) != "a_fifo") @fixtures)"#, false; "complicated directory: filter FIFOs")]
    // We filter all invalid filetypes, so the evaluation has to succeed.
    #[test_case(r#"(builtins.filterSource (p: t: t != "unknown") @fixtures)"#, true; "complicated directory: filter out unsupported types")]
    fn builtins_filter_source_unsupported_files(code: &str, success: bool) {
        use nix::sys::stat;
        use nix::unistd;
        use std::os::unix::net::UnixListener;
        use tempfile::TempDir;

        // We prepare a directory containing some unsupported file nodes:
        // - character device
        // - socket
        // - FIFO
        // and we run the evaluation inside that CWD.
        //
        // block devices cannot be tested because we don't have the right permissions.
        let temp = TempDir::with_prefix("foo").expect("Failed to create a temporary directory");

        // read, write, execute to the owner.
        unistd::mkfifo(&temp.path().join("a_fifo"), stat::Mode::S_IRWXU)
            .expect("Failed to create the FIFO");

        UnixListener::bind(temp.path().join("a_socket")).expect("Failed to create the socket");

        stat::mknod(
            &temp.path().join("a_charnode"),
            stat::SFlag::S_IFCHR,
            stat::Mode::S_IRWXU,
            0,
        )
        .expect("Failed to create a character device node");

        let code_replaced = code.replace("@fixtures", &temp.path().to_string_lossy());
        let eval_result = eval(&code_replaced);

        if success {
            assert!(
                eval_result.value.is_some(),
                "unexpected failure on a directory of unsupported file types but all filtered: {:?}",
                eval_result.errors
            );
        } else {
            assert!(
                eval_result.value.is_none(),
                "unexpected success on unsupported file type ingestion: {:?}",
                eval_result.value
            );
        }
    }
}
