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
    rc::Rc,
};

use crate::{errors::ErrorKind, upvalues::UpvalueCarrier, vm::VM, EvalResult, Value};

use super::Lambda;

/// Internal representation of the different states of a thunk.
#[derive(Debug)]
enum ThunkRepr {
    /// Thunk is closed over some values, suspended and awaiting
    /// execution.
    Suspended {
        lambda: Rc<Lambda>,
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
    pub fn new(lambda: Rc<Lambda>) -> Self {
        Thunk(Rc::new(RefCell::new(ThunkRepr::Suspended {
            upvalues: Vec::with_capacity(lambda.upvalue_count),
            lambda,
        })))
    }

    pub fn force(&self, vm: &mut VM) -> EvalResult<Ref<'_, Value>> {
        // Due to mutable borrowing rules, the following code can't
        // easily use a match statement or something like that; it
        // requires a bit of manual fiddling.
        let mut thunk_mut = self.0.borrow_mut();

        if let ThunkRepr::Blackhole = *thunk_mut {
            return Err(ErrorKind::InfiniteRecursion.into());
        }

        if matches!(*thunk_mut, ThunkRepr::Suspended { .. }) {
            if let ThunkRepr::Suspended { lambda, upvalues } =
                std::mem::replace(&mut *thunk_mut, ThunkRepr::Blackhole)
            {
                vm.call(lambda, upvalues, 0);
                *thunk_mut = ThunkRepr::Evaluated(vm.run()?);
            }
        }

        drop(thunk_mut);

        // Otherwise it's already ThunkRepr::Evaluated and we do not
        // need another branch.

        Ok(Ref::map(self.0.borrow(), |t| match t {
            ThunkRepr::Evaluated(value) => value,
            _ => unreachable!("already evaluated"),
        }))
    }
}

impl UpvalueCarrier for Thunk {
    fn upvalue_count(&self) -> usize {
        if let ThunkRepr::Suspended { lambda, .. } = &*self.0.borrow() {
            return lambda.upvalue_count;
        }

        panic!("upvalues() on non-suspended thunk");
    }

    fn upvalues(&self) -> Ref<'_, [Value]> {
        Ref::map(self.0.borrow(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => upvalues.as_slice(),
            _ => panic!("upvalues() on non-suspended thunk"),
        })
    }

    fn upvalues_mut(&self) -> RefMut<'_, Vec<Value>> {
        RefMut::map(self.0.borrow_mut(), |thunk| match thunk {
            ThunkRepr::Suspended { upvalues, .. } => upvalues,
            _ => panic!("upvalues() on non-suspended thunk"),
        })
    }
}
