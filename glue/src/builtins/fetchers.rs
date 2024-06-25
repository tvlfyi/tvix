//! Contains builtins that fetch paths from the Internet, or local filesystem.

use super::utils::select_string;
use crate::{
    fetchers::{url_basename, Fetch},
    tvix_store_io::TvixStoreIO,
};
use nix_compat::nixhash;
use std::rc::Rc;
use tvix_eval::builtin_macros::builtins;
use tvix_eval::generators::Gen;
use tvix_eval::generators::GenCo;
use tvix_eval::{CatchableErrorKind, ErrorKind, Value};
use url::Url;

// Used as a return type for extract_fetch_args, which is sharing some
// parsing code between the fetchurl and fetchTarball builtins.
struct NixFetchArgs {
    url: Url,
    name: Option<String>,
    sha256: Option<[u8; 32]>,
}

// `fetchurl` and `fetchTarball` accept a single argument, which can either be the URL (as string),
// or an attrset, where `url`, `sha256` and `name` keys are allowed.
async fn extract_fetch_args(
    co: &GenCo,
    args: Value,
) -> Result<Result<NixFetchArgs, CatchableErrorKind>, ErrorKind> {
    if let Ok(url_str) = args.to_str() {
        // Get the raw bytes, not the ToString repr.
        let url_str =
            String::from_utf8(url_str.as_bytes().to_vec()).map_err(|_| ErrorKind::Utf8)?;

        // Parse the URL.
        let url = Url::parse(&url_str).map_err(|e| ErrorKind::TvixError(Rc::new(e)))?;

        return Ok(Ok(NixFetchArgs {
            url,
            name: None,
            sha256: None,
        }));
    }

    let attrs = args.to_attrs().map_err(|_| ErrorKind::TypeError {
        expected: "attribute set or contextless string",
        actual: args.type_of(),
    })?;

    let url_str = match select_string(co, &attrs, "url").await? {
        Ok(s) => s.ok_or_else(|| ErrorKind::AttributeNotFound { name: "url".into() })?,
        Err(cek) => return Ok(Err(cek)),
    };
    let name = match select_string(co, &attrs, "name").await? {
        Ok(s) => s,
        Err(cek) => return Ok(Err(cek)),
    };
    let sha256_str = match select_string(co, &attrs, "sha256").await? {
        Ok(s) => s,
        Err(cek) => return Ok(Err(cek)),
    };

    // TODO: disallow other attrset keys, to match Nix' behaviour.

    // parse the sha256 string into a digest.
    let sha256 = match sha256_str {
        Some(sha256_str) => {
            let nixhash = nixhash::from_str(&sha256_str, Some("sha256"))
                // TODO: DerivationError::InvalidOutputHash should be moved to ErrorKind::InvalidHash and used here instead
                .map_err(|e| ErrorKind::TvixError(Rc::new(e)))?;

            Some(nixhash.digest_as_bytes().try_into().expect("is sha256"))
        }
        None => None,
    };

    // Parse the URL.
    let url = Url::parse(&url_str).map_err(|e| ErrorKind::TvixError(Rc::new(e)))?;

    Ok(Ok(NixFetchArgs { url, name, sha256 }))
}

#[allow(unused_variables)] // for the `state` arg, for now
#[builtins(state = "Rc<TvixStoreIO>")]
pub(crate) mod fetcher_builtins {
    use nix_compat::nixhash::NixHash;

    use super::*;

    /// Consumes a fetch.
    /// If there is enough info to calculate the store path without fetching,
    /// queue the fetch to be fetched lazily, and return the store path.
    /// If there's not enough info to calculate it, do the fetch now, and then
    /// return the store path.
    fn fetch_lazy(state: Rc<TvixStoreIO>, name: String, fetch: Fetch) -> Result<Value, ErrorKind> {
        match fetch
            .store_path(&name)
            .map_err(|e| ErrorKind::TvixError(Rc::new(e)))?
        {
            Some(store_path) => {
                // Move the fetch to KnownPaths, so it can be actually fetched later.
                let sp = state
                    .known_paths
                    .borrow_mut()
                    .add_fetch(fetch, &name)
                    .expect("Tvix bug: should only fail if the store path cannot be calculated");

                debug_assert_eq!(
                    sp, store_path,
                    "calculated store path by KnownPaths should match"
                );

                // Emit the calculated Store Path.
                Ok(Value::Path(Box::new(store_path.to_absolute_path().into())))
            }
            None => {
                // If we don't have enough info, do the fetch now.
                let (store_path, _root_node) = state
                    .tokio_handle
                    .block_on(async { state.fetcher.ingest_and_persist(&name, fetch).await })
                    .map_err(|e| ErrorKind::TvixError(Rc::new(e)))?;

                Ok(Value::Path(Box::new(store_path.to_absolute_path().into())))
            }
        }
    }

    #[builtin("fetchurl")]
    async fn builtin_fetchurl(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        let args = match extract_fetch_args(&co, args).await? {
            Ok(args) => args,
            Err(cek) => return Ok(Value::from(cek)),
        };

        // Derive the name from the URL basename if not set explicitly.
        let name = args
            .name
            .unwrap_or_else(|| url_basename(&args.url).to_owned());

        fetch_lazy(
            state,
            name,
            Fetch::URL {
                url: args.url,
                exp_hash: args.sha256.map(NixHash::Sha256),
            },
        )
    }

    #[builtin("fetchTarball")]
    async fn builtin_fetch_tarball(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        let args = match extract_fetch_args(&co, args).await? {
            Ok(args) => args,
            Err(cek) => return Ok(Value::from(cek)),
        };

        // Name defaults to "source" if not set explicitly.
        const DEFAULT_NAME_FETCH_TARBALL: &str = "source";
        let name = args
            .name
            .unwrap_or_else(|| DEFAULT_NAME_FETCH_TARBALL.to_owned());

        fetch_lazy(
            state,
            name,
            Fetch::Tarball {
                url: args.url,
                exp_nar_sha256: args.sha256,
            },
        )
    }

    #[builtin("fetchGit")]
    async fn builtin_fetch_git(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        Err(ErrorKind::NotImplemented("fetchGit"))
    }
}
