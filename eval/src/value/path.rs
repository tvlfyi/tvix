use path_clean::PathClean;
use std::path::PathBuf;

/// This function should match the behavior of canonPath() in
/// src/libutil/util.cc of cppnix.  Currently it does not match that
/// behavior; it uses the `path_clean` library which is based on the
/// Go standard library
///
/// TODO: make this match the behavior of cppnix
/// TODO: write tests for this

pub fn canon_path(path: PathBuf) -> PathBuf {
    path.clean()
}
