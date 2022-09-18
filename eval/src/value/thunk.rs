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

use std::{
    cell::{Ref, RefCell, RefMut},
    fmt::Display,
    rc::Rc,
};

use crate::{
    errors::ErrorKind,
    upvalues::{UpvalueCarrier, Upvalues},
    vm::VM,
    Value,
};

use super::Lambda;

/// Internal representation of the different states of a thunk.
#[derive(Clone, Debug, PartialEq)]
enum ThunkRepr {
    /// Thunk is closed over some values, suspended and awaiting
    /// execution.
    Suspended {
        lambda: Rc<Lambda>,
        upvalues: Upvalues,
    },

    /// Thunk currently under-evaluation; encountering a blackhole
    /// value means that infinite recursion has occured.
    Blackhole,

    /// Fully evaluated thunk.
    Evaluated(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Thunk(Rc<RefCell<ThunkRepr>>);

impl Thunk {
    pub fn new(lambda: Rc<Lambda>) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Suspended {
            upvalues: Upvalues::with_capacity(lambda.upvalue_count),
            lambda: lambda.clone(),
        })))
    }

    /// Evaluate the content of a thunk, potentially repeatedly, until
    /// a non-thunk value is returned.
    ///
    /// This will change the existing thunk (and thus all references
    /// to it, providing memoization) through interior mutability. In
    /// case of nested thunks, the intermediate thunk representations
    /// are replaced.
    pub fn force(&self, vm: &mut VM) -> Result<(), ErrorKind> {
        loop {
            let mut thunk_mut = self.0.borrow_mut();

            match *thunk_mut {
                ThunkRepr::Evaluated(Value::Thunk(ref inner_thunk)) => {
                    let inner_repr = inner_thunk.0.borrow().clone();
                    *thunk_mut = inner_repr;
                }

                ThunkRepr::Evaluated(_) => return Ok(()),
                ThunkRepr::Blackhole => return Err(ErrorKind::InfiniteRecursion),

                ThunkRepr::Suspended { .. } => {
                    if let ThunkRepr::Suspended { lambda, upvalues } =
                        std::mem::replace(&mut *thunk_mut, ThunkRepr::Blackhole)
                    {
                        drop(thunk_mut);
                        let evaluated = ThunkRepr::Evaluated(
                            vm.call(lambda, upvalues, 0)
                                .map_err(|e| ErrorKind::ThunkForce(Box::new(e)))?,
                        );
                        (*self.0.borrow_mut()) = evaluated;
                    }
                }
            }
        }
    }

    /// Returns a reference to the inner evaluated value of a thunk.
    /// It is an error to call this on a thunk that has not been
    /// forced, or is not otherwise known to be fully evaluated.
    // Note: Due to the interior mutability of thunks this is
    // difficult to represent in the type system without impacting the
    // API too much.
    pub fn value(&self) -> Ref<Value> {
        Ref::map(self.0.borrow(), |thunk| {
            if let ThunkRepr::Evaluated(value) = thunk {
                return value;
            }

            panic!("Thunk::value called on non-evaluated thunk");
        })
    }
}

impl UpvalueCarrier for Thunk {
    fn upvalue_count(&self) -> usize {
        if let ThunkRepr::Suspended { lambda, .. } = &*self.0.borrow() {
            return lambda.upvalue_count;
        }

        panic!("upvalues() on non-suspended thunk");
    }

    fn upvalues(&self) -> Ref<'_, Upvalues> {
        Ref::map(self.0.borrow(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => upvalues,
            _ => panic!("upvalues() on non-suspended thunk"),
        })
    }

    fn upvalues_mut(&self) -> RefMut<'_, Upvalues> {
        RefMut::map(self.0.borrow_mut(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => upvalues,
            _ => panic!("upvalues() on non-suspended thunk"),
        })
    }
}

impl Display for Thunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.try_borrow() {
            Ok(repr) => match &*repr {
                ThunkRepr::Evaluated(v) => v.fmt(f),
                _ => f.write_str("internal[thunk]"),
            },

            _ => f.write_str("internal[thunk]"),
        }
    }
}
