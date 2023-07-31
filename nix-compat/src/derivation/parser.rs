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

fn parse_input_derivations(i: &[u8]) -> NomResult<&[u8], BTreeMap<String, Vec<String>>> {
    parse_kv::<Vec<String>, _>(aterm::parse_str_list)(i)
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
                |i| terminated(aterm::parse_str_list, nomchar(','))(i).map_err(into_nomerror),
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
                // All values in input_derivations need to be converted from
                // Vec<String> to BTreeSet<String>
                let mut input_derivations_new: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
                for (k, v) in input_derivations.into_iter() {
                    let values_new: BTreeSet<_> = BTreeSet::from_iter(v.into_iter());
                    input_derivations_new.insert(k, values_new);
                    // TODO: actually check they're not duplicate in the parser side!
                }

                // Input sources need to be converted from Vec<_> to BTreeSet<_>
                let input_sources_new: BTreeSet<_> = BTreeSet::from_iter(input_sources);

                Derivation {
                    arguments,
                    builder,
                    environment,
                    input_derivations: input_derivations_new,
                    input_sources: input_sources_new,
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
    use std::collections::BTreeMap;

    use crate::derivation::Output;
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
    }

    #[test_case(b"[(\"a\",\"1\"),(\"b\",\"2\")]", &EXP_AB_MAP, b""; "simple")]
    fn parse_kv(input: &'static [u8], expected: &BTreeMap<String, BString>, exp_rest: &[u8]) {
        let (rest, parsed) = super::parse_kv::<BString, _>(crate::aterm::parse_bstr_field)(input)
            .expect("must parse");
        assert_eq!(exp_rest, rest, "expected remainder");
        assert_eq!(*expected, parsed);
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
