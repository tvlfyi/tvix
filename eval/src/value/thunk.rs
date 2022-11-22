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
    collections::HashSet,
    rc::Rc,
};

use codemap::Span;

use crate::{
    errors::{Error, ErrorKind},
    upvalues::Upvalues,
    value::Closure,
    vm::VM,
    Value,
};

use super::{Lambda, TotalDisplay};

/// Internal representation of the different states of a thunk.
///
/// Upvalues must be finalised before leaving the initial state
/// (Suspended or RecursiveClosure).  The [`value()`] function may
/// not be called until the thunk is in the final state (Evaluated).
#[derive(Clone, Debug)]
enum ThunkRepr {
    /// Thunk is closed over some values, suspended and awaiting
    /// execution.
    Suspended {
        lambda: Rc<Lambda>,
        upvalues: Upvalues,
        span: Span,
    },

    /// Thunk currently under-evaluation; encountering a blackhole
    /// value means that infinite recursion has occured.
    Blackhole,

    /// Fully evaluated thunk.
    Evaluated(Value),
}

/// A thunk is created for any value which requires non-strict
/// evaluation due to self-reference or lazy semantics (or both).
/// Every reference cycle involving `Value`s will contain at least
/// one `Thunk`.
#[derive(Clone, Debug)]
pub struct Thunk(Rc<RefCell<ThunkRepr>>);

impl Thunk {
    pub fn new_closure(lambda: Rc<Lambda>) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Evaluated(Value::Closure(
            Closure {
                upvalues: Rc::new(Upvalues::with_capacity(lambda.upvalue_count)),
                lambda: lambda.clone(),
                #[cfg(debug_assertions)]
                is_finalised: false,
            },
        )))))
    }

    pub fn new_suspended(lambda: Rc<Lambda>, span: Span) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Suspended {
            upvalues: Upvalues::with_capacity(lambda.upvalue_count),
            lambda: lambda.clone(),
            span,
        })))
    }

    /// Evaluate the content of a thunk, potentially repeatedly, until a
    /// non-thunk value is returned.
    ///
    /// This will change the existing thunk (and thus all references to it,
    /// providing memoization) through interior mutability. In case of nested
    /// thunks, the intermediate thunk representations are replaced.
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
                    if let ThunkRepr::Suspended {
                        lambda,
                        upvalues,
                        span,
                    } = std::mem::replace(&mut *thunk_mut, ThunkRepr::Blackhole)
                    {
                        drop(thunk_mut);
                        vm.enter_frame(lambda, upvalues, 0)
                            .map_err(|e| ErrorKind::ThunkForce(Box::new(Error { span, ..e })))?;
                        (*self.0.borrow_mut()) = ThunkRepr::Evaluated(vm.pop())
                    }
                }
            }
        }
    }

    pub fn finalise(&self, stack: &[Value]) {
        self.upvalues_mut().resolve_deferred_upvalues(stack);
        #[cfg(debug_assertions)]
        {
            let inner: &mut ThunkRepr = &mut self.0.as_ref().borrow_mut();
            if let ThunkRepr::Evaluated(Value::Closure(c)) = inner {
                c.is_finalised = true;
            }
        }
    }

    pub fn is_evaluated(&self) -> bool {
        matches!(*self.0.borrow(), ThunkRepr::Evaluated(_))
    }

    /// Returns a reference to the inner evaluated value of a thunk.
    /// It is an error to call this on a thunk that has not been
    /// forced, or is not otherwise known to be fully evaluated.
    // Note: Due to the interior mutability of thunks this is
    // difficult to represent in the type system without impacting the
    // API too much.
    pub fn value(&self) -> Ref<Value> {
        Ref::map(self.0.borrow(), |thunk| match thunk {
            ThunkRepr::Evaluated(value) => {
                #[cfg(debug_assertions)]
                if matches!(
                    value,
                    Value::Closure(Closure {
                        is_finalised: false,
                        ..
                    })
                ) {
                    panic!("Thunk::value called on an unfinalised closure");
                }
                return value;
            }
            ThunkRepr::Blackhole => panic!("Thunk::value called on a black-holed thunk"),
            ThunkRepr::Suspended { .. } => panic!("Thunk::value called on a suspended thunk"),
        })
    }

    pub fn upvalues(&self) -> Ref<'_, Upvalues> {
        Ref::map(self.0.borrow(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => upvalues,
            ThunkRepr::Evaluated(Value::Closure(Closure { upvalues, .. })) => upvalues,
            _ => panic!("upvalues() on non-suspended thunk"),
        })
    }

    pub fn upvalues_mut(&self) -> RefMut<'_, Upvalues> {
        RefMut::map(self.0.borrow_mut(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => upvalues,
            ThunkRepr::Evaluated(Value::Closure(Closure {
                upvalues,
                #[cfg(debug_assertions)]
                is_finalised,
                ..
            })) => {
                #[cfg(debug_assertions)]
                if *is_finalised {
                    panic!("Thunk::upvalues_mut() called on a finalised closure");
                }
                Rc::get_mut(upvalues)
                    .expect("upvalues_mut() was called on a thunk which already had multiple references to it")
            }
            thunk => panic!("upvalues() on non-suspended thunk: {thunk:?}"),
        })
    }

    /// Do not use this without first reading and understanding
    /// `tvix/docs/value-pointer-equality.md`.
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl TotalDisplay for Thunk {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        if !set.insert(self) {
            return f.write_str("<CYCLE>");
        }

        match self.0.try_borrow() {
            Ok(repr) => match &*repr {
                ThunkRepr::Evaluated(v) => v.total_fmt(f, set),
                _ => f.write_str("internal[thunk]"),
            },

            _ => f.write_str("internal[thunk]"),
        }
    }
}

/// A wrapper type for tracking which thunks have already been seen in a
/// context. This is necessary for cycle detection.
///
/// The inner `HashSet` is not available on the outside, as it would be
/// potentially unsafe to interact with the pointers in the set.
#[derive(Default)]
pub struct ThunkSet(HashSet<*mut ThunkRepr>);

impl ThunkSet {
    /// Check whether the given thunk has already been seen. Will mark the thunk
    /// as seen otherwise.
    pub fn insert(&mut self, thunk: &Thunk) -> bool {
        let ptr: *mut ThunkRepr = thunk.0.as_ptr();
        self.0.insert(ptr)
    }
}
