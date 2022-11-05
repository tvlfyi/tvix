mod builtins;
mod chunk;
mod compiler;
mod errors;
mod eval;
pub mod observer;
mod opcode;
mod pretty_ast;
mod source;
mod spans;
mod systems;
mod upvalues;
mod value;
mod vm;
mod warnings;

mod nix_search_path;
#[cfg(test)]
mod properties;
#[cfg(test)]
mod test_utils;
#[cfg(test)]
mod tests;

use std::rc::Rc;

// Re-export the public interface used by other crates.
pub use crate::builtins::global_builtins;
pub use crate::compiler::{compile, prepare_globals};
pub use crate::errors::{ErrorKind, EvalResult};
pub use crate::eval::{interpret, Options};
pub use crate::pretty_ast::pretty_print_expr;
pub use crate::source::SourceCode;
pub use crate::value::{Builtin, Value};
pub use crate::vm::{run_lambda, VM};

// TODO: use Rc::unwrap_or_clone once it is stabilised.
// https://doc.rust-lang.org/std/rc/struct.Rc.html#method.unwrap_or_clone
pub fn unwrap_or_clone_rc<T: Clone>(rc: Rc<T>) -> T {
    Rc::try_unwrap(rc).unwrap_or_else(|rc| (*rc).clone())
}
