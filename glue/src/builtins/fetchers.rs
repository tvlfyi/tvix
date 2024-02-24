//! Contains builtins that fetch paths from the Internet

use crate::tvix_store_io::TvixStoreIO;
use bstr::ByteSlice;
use nix_compat::nixhash::{self, CAHash};
use nix_compat::store_path::{build_ca_path, StorePathRef};
use std::rc::Rc;
use tvix_eval::builtin_macros::builtins;
use tvix_eval::generators::GenCo;
use tvix_eval::{CatchableErrorKind, ErrorKind, NixContextElement, NixString, Value};

use super::utils::select_string;
use super::{DerivationError, FetcherError};

/// Attempts to mimic `nix::libutil::baseNameOf`
fn url_basename(s: &str) -> &str {
    if s.is_empty() {
        return "";
    }

    let mut last = s.len() - 1;
    if s.chars().nth(last).unwrap() == '/' && last > 0 {
        last -= 1;
    }

    if last == 0 {
        return "";
    }

    let pos = match s[..=last].rfind('/') {
        Some(pos) => {
            if pos == last - 1 {
                0
            } else {
                pos
            }
        }
        None => 0,
    };

    &s[(pos + 1)..=last]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HashMode {
    Flat,
    Recursive,
}

/// Struct representing the arguments passed to fetcher functions
#[derive(Debug, PartialEq, Eq)]
struct FetchArgs {
    url: String,
    name: String,
    hash: Option<CAHash>,
}

impl FetchArgs {
    pub fn new(
        url: String,
        name: Option<String>,
        sha256: Option<String>,
        mode: HashMode,
    ) -> nixhash::NixHashResult<Self> {
        Ok(Self {
            name: name.unwrap_or_else(|| url_basename(&url).to_owned()),
            url,
            hash: sha256
                .map(|h| {
                    let hash = nixhash::from_str(&h, Some("sha256"))?;
                    Ok(match mode {
                        HashMode::Flat => Some(nixhash::CAHash::Flat(hash)),
                        HashMode::Recursive => Some(nixhash::CAHash::Nar(hash)),
                    })
                })
                .transpose()?
                .flatten(),
        })
    }

    fn store_path(&self) -> Result<Option<StorePathRef>, ErrorKind> {
        let Some(h) = &self.hash else {
            return Ok(None);
        };
        build_ca_path(&self.name, h, Vec::<String>::new(), false)
            .map(Some)
            .map_err(|e| FetcherError::from(e).into())
    }

    async fn extract(
        co: &GenCo,
        args: Value,
        default_name: Option<&str>,
        mode: HashMode,
    ) -> Result<Result<Self, CatchableErrorKind>, ErrorKind> {
        if let Ok(url) = args.to_str() {
            return Ok(Ok(FetchArgs::new(
                url.to_str()?.to_owned(),
                None,
                None,
                mode,
            )
            .map_err(DerivationError::InvalidOutputHash)?));
        }

        let attrs = args.to_attrs().map_err(|_| ErrorKind::TypeError {
            expected: "attribute set or string",
            actual: args.type_of(),
        })?;

        let url = match select_string(co, &attrs, "url").await? {
            Ok(s) => s.ok_or_else(|| ErrorKind::AttributeNotFound { name: "url".into() })?,
            Err(cek) => return Ok(Err(cek)),
        };
        let name = match select_string(co, &attrs, "name").await? {
            Ok(s) => s.or_else(|| default_name.map(|s| s.to_owned())),
            Err(cek) => return Ok(Err(cek)),
        };
        let sha256 = match select_string(co, &attrs, "sha256").await? {
            Ok(s) => s,
            Err(cek) => return Ok(Err(cek)),
        };

        Ok(Ok(
            FetchArgs::new(url, name, sha256, mode).map_err(DerivationError::InvalidOutputHash)?
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchMode {
    Url,
    Tarball,
}

impl From<FetchMode> for HashMode {
    fn from(value: FetchMode) -> Self {
        match value {
            FetchMode::Url => HashMode::Flat,
            FetchMode::Tarball => HashMode::Recursive,
        }
    }
}

impl FetchMode {
    fn default_name(self) -> Option<&'static str> {
        match self {
            FetchMode::Url => None,
            FetchMode::Tarball => Some("source"),
        }
    }
}

fn string_from_store_path(store_path: StorePathRef) -> NixString {
    NixString::new_context_from(
        NixContextElement::Plain(store_path.to_absolute_path()).into(),
        store_path.to_absolute_path(),
    )
}

async fn fetch(
    state: Rc<TvixStoreIO>,
    co: GenCo,
    args: Value,
    mode: FetchMode,
) -> Result<Value, ErrorKind> {
    let args = match FetchArgs::extract(&co, args, mode.default_name(), mode.into()).await? {
        Ok(args) => args,
        Err(cek) => return Ok(cek.into()),
    };

    if let Some(store_path) = args.store_path()? {
        if state.store_path_exists(store_path).await? {
            return Ok(string_from_store_path(store_path).into());
        }
    }

    let ca = args.hash;
    let store_path = Rc::clone(&state).tokio_handle.block_on(async move {
        match mode {
            FetchMode::Url => {
                state
                    .fetch_url(
                        &args.url,
                        &args.name,
                        ca.as_ref().map(|c| c.hash().into_owned()).as_ref(),
                    )
                    .await
            }
            FetchMode::Tarball => state.fetch_tarball(&args.url, &args.name, ca).await,
        }
    })?;

    Ok(string_from_store_path(store_path.as_ref()).into())
}

#[allow(unused_variables)] // for the `state` arg, for now
#[builtins(state = "Rc<TvixStoreIO>")]
pub(crate) mod fetcher_builtins {
    use super::*;

    use tvix_eval::generators::Gen;

    #[builtin("fetchurl")]
    async fn builtin_fetchurl(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        fetch(state, co, args, FetchMode::Url).await
    }

    #[builtin("fetchTarball")]
    async fn builtin_fetch_tarball(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        fetch(state, co, args, FetchMode::Tarball).await
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nix_compat::store_path::StorePath;

    use super::*;

    #[test]
    fn fetchurl_store_path() {
        let url = "https://raw.githubusercontent.com/aaptel/notmuch-extract-patch/f732a53e12a7c91a06755ebfab2007adc9b3063b/notmuch-extract-patch";
        let sha256 = "0nawkl04sj7psw6ikzay7kydj3dhd0fkwghcsf5rzaw4bmp4kbax";
        let args = FetchArgs::new(url.into(), None, Some(sha256.into()), HashMode::Flat).unwrap();

        assert_eq!(
            args.store_path().unwrap().unwrap().to_owned(),
            StorePath::from_str("06qi00hylriyfm0nl827crgjvbax84mz-notmuch-extract-patch").unwrap()
        )
    }

    #[test]
    fn fetch_tarball_store_path() {
        let url = "https://github.com/NixOS/nixpkgs/archive/91050ea1e57e50388fa87a3302ba12d188ef723a.tar.gz";
        let sha256 = "1hf6cgaci1n186kkkjq106ryf8mmlq9vnwgfwh625wa8hfgdn4dm";
        let args = FetchArgs::new(
            url.into(),
            Some("source".into()),
            Some(sha256.into()),
            HashMode::Recursive,
        )
        .unwrap();

        assert_eq!(
            args.store_path().unwrap().unwrap().to_owned(),
            StorePath::from_str("7adgvk5zdfq4pwrhsm3n9lzypb12gw0g-source").unwrap()
        )
    }

    mod url_basename {
        use super::*;

        #[test]
        fn empty_path() {
            assert_eq!(url_basename(""), "");
        }

        #[test]
        fn path_on_root() {
            assert_eq!(url_basename("/dir"), "dir");
        }

        #[test]
        fn relative_path() {
            assert_eq!(url_basename("dir/foo"), "foo");
        }

        #[test]
        fn root_with_trailing_slash() {
            assert_eq!(url_basename("/"), "");
        }

        #[test]
        fn trailing_slash() {
            assert_eq!(url_basename("/dir/"), "dir");
        }
    }
}
