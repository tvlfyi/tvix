//! Contains builtins that deal with the store or builder.
use std::{cell::RefCell, rc::Rc};

use crate::known_paths::KnownPaths;

mod derivation;
mod derivation_error;

pub use derivation_error::Error as DerivationError;

/// Adds derivation-related builtins to the passed [tvix_eval::Evaluation].
///
/// These are `derivation` and `derivationStrict`.
///
/// As they need to interact with `known_paths`, we also need to pass in
/// `known_paths`.
pub fn add_derivation_builtins(
    eval: &mut tvix_eval::Evaluation,
    known_paths: Rc<RefCell<KnownPaths>>,
) {
    eval.builtins
        .extend(derivation::derivation_builtins::builtins(known_paths));

    // Add the actual `builtins.derivation` from compiled Nix code
    eval.src_builtins
        .push(("derivation", include_str!("derivation.nix")));
}

#[cfg(test)]
mod tests {
    use super::add_derivation_builtins;
    use crate::known_paths::KnownPaths;
    use nix_compat::store_path::hash_placeholder;
    use std::{cell::RefCell, rc::Rc};
    use test_case::test_case;
    use tvix_eval::EvaluationResult;

    /// evaluates a given nix expression and returns the result.
    /// Takes care of setting up the evaluator so it knows about the
    // `derivation` builtin.
    fn eval(str: &str) -> EvaluationResult {
        let mut eval = tvix_eval::Evaluation::new_impure();

        let known_paths: Rc<RefCell<KnownPaths>> = Default::default();

        add_derivation_builtins(&mut eval, known_paths.clone());

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
