//! This module implements the runtime representation of a Nix
//! builtin.
//!
//! Builtins are directly backed by Rust code operating on Nix values.

use crate::{errors::ErrorKind, vm::VM};

use super::Value;

use std::{
    fmt::{Debug, Display},
    rc::Rc,
};

/// Trait for closure types of builtins implemented directly by
/// backing Rust code.
///
/// Builtins declare their arity and are passed a vector with the
/// right number of arguments. Additionally, as they might have to
/// force the evaluation of thunks, they are passed a reference to the
/// current VM which they can use for forcing a value.
///
/// Errors returned from a builtin will be annotated with the location
/// of the call to the builtin.
pub trait BuiltinFn: Fn(Vec<Value>, &mut VM) -> Result<Value, ErrorKind> {}
impl<F: Fn(Vec<Value>, &mut VM) -> Result<Value, ErrorKind>> BuiltinFn for F {}

/// Represents a single built-in function which directly executes Rust
/// code that operates on a Nix value.
///
/// Builtins are the only functions in Nix that have varying arities
/// (for example, `hasAttr` has an arity of 2, but `isAttrs` an arity
/// of 1). To facilitate this generically, builtins expect to be
/// called with a vector of Nix values corresponding to their
/// arguments in order.
///
/// Partially applied builtins act similar to closures in that they
/// "capture" the partially applied arguments, and are treated
/// specially when printing their representation etc.
#[derive(Clone)]
pub struct Builtin {
    name: &'static str,
    /// Array reference that describes how many arguments there are (usually 1
    /// or 2) and whether they need to be forced. `true` causes the
    /// corresponding argument to be forced before `func` is called.
    strict_args: &'static [bool],
    func: Rc<dyn BuiltinFn>,

    /// Partially applied function arguments.
    partials: Vec<Value>,
}

impl Builtin {
    pub fn new<F: BuiltinFn + 'static>(
        name: &'static str,
        strict_args: &'static [bool],
        func: F,
    ) -> Self {
        Builtin {
            name,
            strict_args,
            func: Rc::new(func),
            partials: vec![],
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Apply an additional argument to the builtin, which will either
    /// lead to execution of the function or to returning a partial
    /// builtin.
    pub fn apply(mut self, vm: &mut VM, arg: Value) -> Result<Value, ErrorKind> {
        self.partials.push(arg);

        if self.partials.len() == self.strict_args.len() {
            for (idx, force) in self.strict_args.iter().enumerate() {
                if *force {
                    self.partials[idx].force(vm)?;
                }
            }
            return (self.func)(self.partials, vm);
        }

        // Function is not yet ready to be called.
        Ok(Value::Builtin(self))
    }
}

impl Debug for Builtin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "builtin[{}]", self.name)
    }
}

impl Display for Builtin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.partials.is_empty() {
            f.write_str("<<primop-app>>")
        } else {
            f.write_str("<<primop>>")
        }
    }
}

/// Builtins are uniquely identified by their name
impl PartialEq for Builtin {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
