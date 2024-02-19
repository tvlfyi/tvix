//! This contains the code translating from a `builtin:derivation` [Derivation]
//! to a [Fetch].
use crate::fetchers::Fetch;
use nix_compat::{derivation::Derivation, nixhash::CAHash};
use tracing::instrument;
use url::Url;

/// Takes a derivation produced by a call to `builtin:fetchurl` and returns the
/// synthesized [Fetch] for it, as well as the name.
#[instrument]
pub(crate) fn fetchurl_derivation_to_fetch(drv: &Derivation) -> Result<(String, Fetch), Error> {
    if drv.builder != "builtin:fetchurl" {
        return Err(Error::BuilderInvalid);
    }
    if !drv.arguments.is_empty() {
        return Err(Error::ArgumentsInvalud);
    }
    if drv.system != "builtin" {
        return Err(Error::SystemInvalid);
    }

    // ensure this is a fixed-output derivation
    if drv.outputs.len() != 1 {
        return Err(Error::NoFOD);
    }
    let out_output = &drv.outputs.get("out").ok_or(Error::NoFOD)?;
    let ca_hash = out_output.ca_hash.clone().ok_or(Error::NoFOD)?;

    let name: String = drv
        .environment
        .get("name")
        .ok_or(Error::NameMissing)?
        .to_owned()
        .try_into()
        .map_err(|_| Error::NameInvalid)?;

    let url: Url = std::str::from_utf8(drv.environment.get("url").ok_or(Error::URLMissing)?)
        .map_err(|_| Error::URLInvalid)?
        .parse()
        .map_err(|_| Error::URLInvalid)?;

    match ca_hash {
        CAHash::Flat(hash) => {
            return Ok((
                name,
                Fetch::URL {
                    url,
                    exp_hash: Some(hash),
                },
            ))
        }
        CAHash::Nar(hash) => {
            if drv.environment.get("executable").map(|v| v.as_slice()) == Some(b"1") {
                Ok((name, Fetch::Executable { url, hash }))
            } else {
                Ok((name, Fetch::NAR { url, hash }))
            }
        }
        // you can't construct derivations containing this
        CAHash::Text(_) => panic!("Tvix bug: got CaHash::Text in drv"),
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("Invalid builder")]
    BuilderInvalid,
    #[error("invalid arguments")]
    ArgumentsInvalud,
    #[error("Invalid system")]
    SystemInvalid,
    #[error("Derivation is not fixed-output")]
    NoFOD,
    #[error("Missing URL")]
    URLMissing,
    #[error("Invalid URL")]
    URLInvalid,
    #[error("Missing Name")]
    NameMissing,
    #[error("Name invalid")]
    NameInvalid,
}
