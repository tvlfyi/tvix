//! This module encapsulates some logic for upvalue handling, which is
//! relevant to both thunks (delayed computations for lazy-evaluation)
//! as well as closures (lambdas that capture variables from the
//! surrounding scope).

use std::{
    cell::{Ref, RefMut},
    ops::Index,
};

use crate::{opcode::UpvalueIdx, Value};

/// Structure for carrying upvalues inside of thunks & closures. The
/// implementation of this struct encapsulates the logic for capturing
/// and accessing upvalues.
#[derive(Clone, Debug)]
pub struct Upvalues {
    upvalues: Vec<Value>,
}

impl Upvalues {
    pub fn with_capacity(count: usize) -> Self {
        Upvalues {
            upvalues: Vec::with_capacity(count),
        }
    }

    /// Push an upvalue at the end of the upvalue list.
    pub fn push(&mut self, value: Value) {
        self.upvalues.push(value);
    }
}

impl Index<UpvalueIdx> for Upvalues {
    type Output = Value;

    fn index(&self, index: UpvalueIdx) -> &Self::Output {
        &self.upvalues[index.0]
    }
}

/// `UpvalueCarrier` is implemented by all types that carry upvalues.
pub trait UpvalueCarrier {
    fn upvalue_count(&self) -> usize;

    /// Read-only accessor for the stored upvalues.
    fn upvalues(&self) -> Ref<'_, Upvalues>;

    /// Mutable accessor for stored upvalues.
    fn upvalues_mut(&self) -> RefMut<'_, Upvalues>;

    /// Read an upvalue at the given index.
    fn upvalue(&self, idx: UpvalueIdx) -> Ref<'_, Value> {
        Ref::map(self.upvalues(), |v| &v.upvalues[idx.0])
    }

    /// Resolve deferred upvalues from the provided stack slice,
    /// mutating them in the internal upvalue slots.
    fn resolve_deferred_upvalues(&self, stack: &[Value]) {
        for upvalue in self.upvalues_mut().upvalues.iter_mut() {
            if let Value::DeferredUpvalue(idx) = upvalue {
                *upvalue = stack[idx.0].clone();
            }
        }
    }
}
