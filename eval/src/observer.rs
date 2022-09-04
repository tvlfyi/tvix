//! Implements a trait for things that wish to observe internal state
//! changes of tvix-eval.
//!
//! This can be used to gain insights from compilation, to trace the
//! runtime, and so on.

use crate::value::Lambda;

use std::rc::Rc;

/// Implemented by types that wish to observe internal happenings of
/// Tvix.
///
/// All methods are optional, that is, observers can implement only
/// what they are interested in observing.
pub trait Observer {
    /// Called when the compiler finishes compilation of the top-level
    /// of an expression (usually the root Nix expression of a file).
    fn observe_compiled_toplevel(&mut self, _: &Rc<Lambda>) {}

    /// Called when the compiler finishes compilation of a
    /// user-defined function.
    ///
    /// Note that in Nix there are only single argument functions, so
    /// in an expression like `a: b: c: ...` this method will be
    /// called three times.
    fn observe_compiled_lambda(&mut self, _: &Rc<Lambda>) {}

    /// Called when the compiler finishes compilation of a thunk.
    fn observe_compiled_thunk(&mut self, _: &Rc<Lambda>) {}
}

#[derive(Default)]
pub struct NoOpObserver {}

impl Observer for NoOpObserver {}
