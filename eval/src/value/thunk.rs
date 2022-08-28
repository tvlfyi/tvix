//! This module implements the runtime representation of Thunks.
//!
//! Thunks are a special kind of Nix value, similar to a 0-argument
//! closure that yields some value. Thunks are used to implement the
//! lazy evaluation behaviour of Nix:
//!
//! Whenever the compiler determines that an expression should be
//! evaluated lazily, it creates a thunk instead of compiling the
//! expression value directly. At any point in the runtime where the
//! actual value of a thunk is required, it is "forced", meaning that
//! the encompassing computation takes place and the thunk takes on
//! its new value.
//!
//! Thunks have interior mutability to be able to memoise their
//! computation. Once a thunk is evaluated, its internal
//! representation becomes the result of the expression. It is legal
//! for the runtime to replace a thunk object directly with its value
//! object, but when forcing a thunk, the runtime *must* mutate the
//! memoisable slot.

use std::{cell::RefCell, rc::Rc};

use crate::Value;

use super::Lambda;

/// Internal representation of the different states of a thunk.
#[derive(Debug)]
enum ThunkRepr {
    /// Thunk is suspended and awaiting execution.
    Suspended { lambda: Lambda },

    /// Thunk is closed over some values, suspended and awaiting
    /// execution.
    ClosedSuspended {
        lambda: Lambda,
        upvalues: Vec<Value>,
    },

    /// Thunk currently under-evaluation; encountering a blackhole
    /// value means that infinite recursion has occured.
    Blackhole,

    /// Fully evaluated thunk.
    Evaluated(Value),
}

#[derive(Clone, Debug)]
pub struct Thunk(Rc<RefCell<ThunkRepr>>);

impl Thunk {
    pub fn new(lambda: Lambda) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Suspended { lambda })))
    }
}
