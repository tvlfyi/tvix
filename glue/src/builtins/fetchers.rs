//! Contains builtins that fetch paths from the Internet

use crate::tvix_store_io::TvixStoreIO;
use std::rc::Rc;
use tvix_eval::builtin_macros::builtins;
use tvix_eval::Value;

#[allow(unused_variables)] // for the `state` arg, for now
#[builtins(state = "Rc<TvixStoreIO>")]
pub(crate) mod fetcher_builtins {
    use super::*;

    use tvix_eval::generators::Gen;
    use tvix_eval::{generators::GenCo, ErrorKind};

    #[builtin("fetchurl")]
    async fn builtin_fetchurl(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        url: Value,
    ) -> Result<Value, ErrorKind> {
        Err(ErrorKind::NotImplemented("fetchurl"))
    }

    #[builtin("fetchTarball")]
    async fn builtin_fetch_tarball(
        state: Rc<TvixStoreIO>,
        co: GenCo,
        args: Value,
    ) -> Result<Value, ErrorKind> {
        Err(ErrorKind::NotImplemented("fetchTarball"))
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
