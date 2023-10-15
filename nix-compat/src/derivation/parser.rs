//! This module constructs a [Derivation] by parsing its [ATerm][]
//! serialization.
//!
//! [ATerm]: http://program-transformation.org/Tools/ATermFormat.html

use bstr::BString;
use nom::bytes::complete::tag;
use nom::character::complete::char as nomchar;
use nom::combinator::{all_consuming, map_res};
use nom::multi::{separated_list0, separated_list1};
use nom::sequence::{delimited, preceded, separated_pair, terminated, tuple};
use std::collections::{BTreeMap, BTreeSet};
use thiserror;

use super::parse_error::{into_nomerror, ErrorKind, NomError, NomResult};
use super::{write, Derivation, NixHashWithMode, Output};
use crate::{aterm, nixhash};

#[derive(Debug, thiserror::Error)]
pub enum Error<I> {
    #[error("parsing error: {0}")]
    ParseError(NomError<I>),
    #[error("premature EOF")]
    Incomplete,
    #[error("validation error: {0}")]
    ValidationError(super::DerivationError),
}

pub(crate) fn parse(i: &[u8]) -> Result<Derivation, Error<&[u8]>> {
    match all_consuming(parse_derivation)(i) {
        Ok((rest, derivation)) => {
            // this shouldn't happen, as all_consuming shouldn't return.
            debug_assert!(rest.is_empty());

            // invoke validate
            derivation.validate(true).map_err(Error::ValidationError)?;

            Ok(derivation)
        }
        Err(nom::Err::Incomplete(_)) => Err(Error::Incomplete),
        Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(Error::ParseError(e)),
    }
}

/// Parse one output in ATerm. This is 4 string fields inside parans:
/// output name, output path, algo (and mode), digest.
/// Returns the output name and [Output] struct.
fn parse_output(i: &[u8]) -> NomResult<&[u8], (String, Output)> {
    delimited(
        nomchar('('),
        map_res(
            |i| {
                tuple((
                    terminated(aterm::parse_string_field, nomchar(',')),
                    terminated(aterm::parse_string_field, nomchar(',')),
                    terminated(aterm::parse_string_field, nomchar(',')),
                    aterm::parse_bstr_field,
                ))(i)
                .map_err(into_nomerror)
            },
            |(output_name, output_path, algo_and_mode, encoded_digest)| {
                // convert these 4 fields into an [Output].
                let hash_with_mode_res = {
                    if algo_and_mode.is_empty() && encoded_digest.is_empty() {
                        None
                    } else {
                        match data_encoding::HEXLOWER.decode(&encoded_digest) {
                            Ok(digest) => Some(NixHashWithMode::from_algo_mode_hash(
                                &algo_and_mode,
                                &digest,
                            )),
                            Err(e) => Some(Err(nixhash::Error::InvalidBase64Encoding(e))),
                        }
                    }
                }
                .transpose();

                match hash_with_mode_res {
                    Ok(hash_with_mode) => Ok((
                        output_name,
                        Output {
                            path: output_path,
                            hash_with_mode,
                        },
                    )),
                    Err(e) => Err(nom::Err::Failure(NomError {
                        input: i,
                        code: ErrorKind::NixHashError(e),
                    })),
                }
            },
        ),
        nomchar(')'),
    )(i)
}

/// Parse multiple outputs in ATerm. This is a list of things acccepted by
/// parse_output, and takes care of turning the (String, Output) returned from
/// it to a BTreeMap.
/// We don't use parse_kv here, as it's dealing with 2-tuples, and these are
/// 4-tuples.
fn parse_outputs(i: &[u8]) -> NomResult<&[u8], BTreeMap<String, Output>> {
    let res = delimited(
        nomchar('['),
        separated_list1(tag(","), parse_output),
        nomchar(']'),
    )(i);

    match res {
        Ok((rst, outputs_lst)) => {
            let mut outputs: BTreeMap<String, Output> = BTreeMap::default();
            for (output_name, output) in outputs_lst.into_iter() {
                if outputs.contains_key(&output_name) {
                    return Err(nom::Err::Failure(NomError {
                        input: i,
                        code: ErrorKind::DuplicateMapKey(output_name),
                    }));
                }
                outputs.insert(output_name, output);
            }
            Ok((rst, outputs))
        }
        // pass regular parse errors along
        Err(e) => Err(e),
    }
}

fn parse_input_derivations(i: &[u8]) -> NomResult<&[u8], BTreeMap<String, BTreeSet<String>>> {
    let (i, input_derivations_list) = parse_kv::<Vec<String>, _>(aterm::parse_str_list)(i)?;

    // This is a HashMap of drv paths to a list of output names.
    let mut input_derivations: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (input_derivation, output_names) in input_derivations_list {
        let mut new_output_names = BTreeSet::new();
        for output_name in output_names.into_iter() {
            if !new_output_names.insert(output_name.clone()) {
                return Err(nom::Err::Failure(NomError {
                    input: i,
                    code: ErrorKind::DuplicateInputDerivationOutputName(
                        input_derivation.to_string(),
                        output_name.to_string(),
                    ),
                }));
            }
        }
        input_derivations.insert(input_derivation, new_output_names);
    }

    Ok((i, input_derivations))
}

fn parse_input_sources(i: &[u8]) -> NomResult<&[u8], BTreeSet<String>> {
    let (i, input_sources_lst) = aterm::parse_str_list(i).map_err(into_nomerror)?;

    let mut input_sources: BTreeSet<_> = BTreeSet::new();
    for input_source in input_sources_lst.into_iter() {
        if !input_sources.insert(input_source.clone()) {
            return Err(nom::Err::Failure(NomError {
                input: i,
                code: ErrorKind::DuplicateInputSource(input_source),
            }));
        }
    }

    Ok((i, input_sources))
}

pub fn parse_derivation(i: &[u8]) -> NomResult<&[u8], Derivation> {
    use nom::Parser;
    preceded(
        tag(write::DERIVATION_PREFIX),
        delimited(
            // inside parens
            nomchar('('),
            // tuple requires all errors to be of the same type, so we need to be a
            // bit verbose here wrapping generic IResult into [NomATermResult].
            tuple((
                // parse outputs
                terminated(parse_outputs, nomchar(',')),
                // // parse input derivations
                terminated(parse_input_derivations, nomchar(',')),
                // // parse input sources
                terminated(parse_input_sources, nomchar(',')),
                // // parse system
                |i| terminated(aterm::parse_string_field, nomchar(','))(i).map_err(into_nomerror),
                // // parse builder
                |i| terminated(aterm::parse_string_field, nomchar(','))(i).map_err(into_nomerror),
                // // parse arguments
                |i| terminated(aterm::parse_str_list, nomchar(','))(i).map_err(into_nomerror),
                // parse environment
                parse_kv::<BString, _>(aterm::parse_bstr_field),
            )),
            nomchar(')'),
        )
        .map(
            |(
                outputs,
                input_derivations,
                input_sources,
                system,
                builder,
                arguments,
                environment,
            )| {
                Derivation {
                    arguments,
                    builder,
                    environment,
                    input_derivations,
                    input_sources,
                    outputs,
                    system,
                }
            },
        ),
    )(i)
}

/// Parse a list of key/value pairs into a BTreeMap.
/// The parser for the values can be passed in.
/// In terms of ATerm, this is just a 2-tuple,
/// but we have the additional restriction that the first element needs to be
/// unique across all tuples.
pub(crate) fn parse_kv<'a, V, VF>(
    vf: VF,
) -> impl FnMut(&'a [u8]) -> NomResult<&'a [u8], BTreeMap<String, V>> + 'static
where
    VF: FnMut(&'a [u8]) -> nom::IResult<&'a [u8], V, nom::error::Error<&'a [u8]>> + Clone + 'static,
{
    move |i|
    // inside brackets
    delimited(
        nomchar('['),
        |ii| {
            let res = separated_list0(
                nomchar(','),
                // inside parens
                delimited(
                    nomchar('('),
                    separated_pair(
                        aterm::parse_string_field,
                        nomchar(','),
                        vf.clone(),
                    ),
                    nomchar(')'),
                ),
            )(ii).map_err(into_nomerror);

            match res {
                Ok((rest, pairs)) => {
                    let mut kvs: BTreeMap<String, V> = BTreeMap::new();
                    for (k, v) in pairs.into_iter() {
                        // collect the 2-tuple to a BTreeMap,
                        // and fail if the key was already seen before.
                        if kvs.insert(k.clone(), v).is_some() {
                            return Err(nom::Err::Failure(NomError {
                                input: i,
                                code: ErrorKind::DuplicateMapKey(k),
                            }));
                        }
                    }
                    Ok((rest, kvs))
                }
                Err(e) => Err(e),
            }
        },
        nomchar(']'),
    )(i)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use crate::derivation::{parse_error::ErrorKind, Output};
    use bstr::{BString, ByteSlice};
    use lazy_static::lazy_static;
    use test_case::test_case;

    lazy_static! {
        static ref EXP_MULTI_OUTPUTS: BTreeMap<String, Output> = {
            let mut b = BTreeMap::new();
            b.insert(
                "lib".to_string(),
                Output {
                    path: "/nix/store/2vixb94v0hy2xc6p7mbnxxcyc095yyia-has-multi-out-lib"
                        .to_string(),
                    hash_with_mode: None,
                },
            );
            b.insert(
                "out".to_string(),
                Output {
                    path: "/nix/store/55lwldka5nyxa08wnvlizyqw02ihy8ic-has-multi-out".to_string(),
                    hash_with_mode: None,
                },
            );
            b
        };
        static ref EXP_AB_MAP: BTreeMap<String, BString> = {
            let mut b = BTreeMap::new();
            b.insert("a".to_string(), b"1".as_bstr().to_owned());
            b.insert("b".to_string(), b"2".as_bstr().to_owned());
            b
        };
        static ref EXP_INPUT_DERIVATIONS_SIMPLE: BTreeMap<String, BTreeSet<String>> = {
            let mut b = BTreeMap::new();
            b.insert(
                "/nix/store/8bjm87p310sb7r2r0sg4xrynlvg86j8k-hello-2.12.1.tar.gz.drv".to_string(),
                {
                    let mut output_names = BTreeSet::new();
                    output_names.insert("out".to_string());
                    output_names
                },
            );
            b.insert(
                "/nix/store/p3jc8aw45dza6h52v81j7lk69khckmcj-bash-5.2-p15.drv".to_string(),
                {
                    let mut output_names = BTreeSet::new();
                    output_names.insert("out".to_string());
                    output_names.insert("lib".to_string());
                    output_names
                },
            );
            b
        };
        static ref EXP_INPUT_DERIVATIONS_SIMPLE_ATERM: String = {
            format!(
                "[(\"{0}\",[\"out\"]),(\"{1}\",[\"out\",\"lib\"])]",
                "/nix/store/8bjm87p310sb7r2r0sg4xrynlvg86j8k-hello-2.12.1.tar.gz.drv",
                "/nix/store/p3jc8aw45dza6h52v81j7lk69khckmcj-bash-5.2-p15.drv"
            )
        };
        static ref EXP_INPUT_SOURCES_SIMPLE: BTreeSet<String> = {
            let mut b = BTreeSet::new();
            b.insert("/nix/store/55lwldka5nyxa08wnvlizyqw02ihy8ic-has-multi-out".to_string());
            b.insert("/nix/store/2vixb94v0hy2xc6p7mbnxxcyc095yyia-has-multi-out-lib".to_string());
            b
        };
    }

    /// Ensure parsing KVs works
    #[test_case(b"[]", &BTreeMap::new(), b""; "empty")]
    #[test_case(b"[(\"a\",\"1\"),(\"b\",\"2\")]", &EXP_AB_MAP, b""; "simple")]
    fn parse_kv(input: &'static [u8], expected: &BTreeMap<String, BString>, exp_rest: &[u8]) {
        let (rest, parsed) = super::parse_kv::<BString, _>(crate::aterm::parse_bstr_field)(input)
            .expect("must parse");
        assert_eq!(exp_rest, rest, "expected remainder");
        assert_eq!(*expected, parsed);
    }

    /// Ensures the kv parser complains about duplicate map keys
    #[test]
    fn parse_kv_fail_dup_keys() {
        let input: &'static [u8] = b"[(\"a\",\"1\"),(\"a\",\"2\")]";
        let e = super::parse_kv::<BString, _>(crate::aterm::parse_bstr_field)(input)
            .expect_err("must fail");

        match e {
            nom::Err::Failure(e) => {
                assert_eq!(ErrorKind::DuplicateMapKey("a".to_string()), e.code);
            }
            _ => panic!("unexpected error"),
        }
    }

    /// Ensure parsing input derivations works.
    #[test_case(b"[]", &BTreeMap::new(); "empty")]
    #[test_case(EXP_INPUT_DERIVATIONS_SIMPLE_ATERM.as_bytes(), &EXP_INPUT_DERIVATIONS_SIMPLE; "simple")]
    fn parse_input_derivations(
        input: &'static [u8],
        expected: &BTreeMap<String, BTreeSet<String>>,
    ) {
        let (rest, parsed) = super::parse_input_derivations(input).expect("must parse");

        assert_eq!(expected, &parsed, "parsed mismatch");
        assert!(rest.is_empty(), "rest must be empty");
    }

    /// Ensures the input derivation parser complains about duplicate output names
    #[test]
    fn parse_input_derivations_fail_dup_output_names() {
        let input_str = format!(
            "[(\"{0}\",[\"out\"]),(\"{1}\",[\"out\",\"out\"])]",
            "/nix/store/8bjm87p310sb7r2r0sg4xrynlvg86j8k-hello-2.12.1.tar.gz.drv",
            "/nix/store/p3jc8aw45dza6h52v81j7lk69khckmcj-bash-5.2-p15.drv"
        );
        let e = super::parse_input_derivations(input_str.as_bytes()).expect_err("must fail");

        match e {
            nom::Err::Failure(e) => {
                assert_eq!(
                    ErrorKind::DuplicateInputDerivationOutputName(
                        "/nix/store/p3jc8aw45dza6h52v81j7lk69khckmcj-bash-5.2-p15.drv".to_string(),
                        "out".to_string()
                    ),
                    e.code
                );
            }
            _ => panic!("unexpected error"),
        }
    }

    /// Ensure parsing input sources works
    #[test_case(b"[]", &BTreeSet::new(); "empty")]
    #[test_case(b"[\"/nix/store/55lwldka5nyxa08wnvlizyqw02ihy8ic-has-multi-out\",\"/nix/store/2vixb94v0hy2xc6p7mbnxxcyc095yyia-has-multi-out-lib\"]", &EXP_INPUT_SOURCES_SIMPLE; "simple")]
    fn parse_input_sources(input: &'static [u8], expected: &BTreeSet<String>) {
        let (rest, parsed) = super::parse_input_sources(input).expect("must parse");

        assert_eq!(expected, &parsed, "parsed mismatch");
        assert!(rest.is_empty(), "rest must be empty");
    }

    /// Ensures the input sources parser complains about duplicate input sources
    #[test]
    fn parse_input_sources_fail_dup_keys() {
        let input: &'static [u8] = b"[\"/nix/store/55lwldka5nyxa08wnvlizyqw02ihy8ic-foo\",\"/nix/store/55lwldka5nyxa08wnvlizyqw02ihy8ic-foo\"]";
        let e = super::parse_input_sources(input).expect_err("must fail");

        match e {
            nom::Err::Failure(e) => {
                assert_eq!(
                    ErrorKind::DuplicateInputSource(
                        "/nix/store/55lwldka5nyxa08wnvlizyqw02ihy8ic-foo".to_string()
                    ),
                    e.code
                );
            }
            _ => panic!("unexpected error"),
        }
    }

    #[test_case(
        br#"("out","/nix/store/5vyvcwah9l9kf07d52rcgdk70g2f4y13-foo","","")"#,
        ("out".to_string(), Output {
            path: "/nix/store/5vyvcwah9l9kf07d52rcgdk70g2f4y13-foo".to_string(),
            hash_with_mode: None
        }); "simple"
    )]
    #[test_case(
        br#"("out","/nix/store/4q0pg5zpfmznxscq3avycvf9xdvx50n3-bar","r:sha256","08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba")"#,
        ("out".to_string(), Output {
            path: "/nix/store/4q0pg5zpfmznxscq3avycvf9xdvx50n3-bar".to_string(),
            hash_with_mode: Some(crate::derivation::NixHashWithMode::Recursive(
                crate::nixhash::from_algo_and_digest (
                   crate::nixhash::HashAlgo::Sha256,
                   &data_encoding::HEXLOWER.decode(b"08813cbee9903c62be4c5027726a418a300da4500b2d369d3af9286f4815ceba").unwrap()
                ).unwrap()
            )),
        }); "fod"
     )]
    fn parse_output(input: &[u8], expected: (String, Output)) {
        let (rest, parsed) = super::parse_output(input).expect("must parse");
        assert!(rest.is_empty());
        assert_eq!(expected, parsed);
    }

    #[test_case(
        br#"[("lib","/nix/store/2vixb94v0hy2xc6p7mbnxxcyc095yyia-has-multi-out-lib","",""),("out","/nix/store/55lwldka5nyxa08wnvlizyqw02ihy8ic-has-multi-out","","")]"#,
        &EXP_MULTI_OUTPUTS;
        "multi-out"
    )]
    fn parse_outputs(input: &[u8], expected: &BTreeMap<String, Output>) {
        let (rest, parsed) = super::parse_outputs(input).expect("must parse");
        assert!(rest.is_empty());
        assert_eq!(*expected, parsed);
    }
}
