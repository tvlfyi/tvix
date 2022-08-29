//! This module encapsulates some logic for upvalue handling, which is
//! relevant to both thunks (delayed computations for lazy-evaluation)
//! as well as closures (lambdas that capture variables from the
//! surrounding scope).

use std::cell::{Ref, RefMut};

use crate::{opcode::UpvalueIdx, Value};

/// `UpvalueCarrier` is implemented by all types that carry upvalues.
pub trait UpvalueCarrier {
    fn upvalue_count(&self) -> usize;

    /// Read-only accessor for the stored upvalues.
    fn upvalues(&self) -> Ref<'_, [Value]>;

    /// Mutable accessor for stored upvalues.
    fn upvalues_mut(&self) -> RefMut<'_, Vec<Value>>;

    /// Read an upvalue at the given index.
    fn upvalue(&self, idx: UpvalueIdx) -> Ref<'_, Value> {
        Ref::map(self.upvalues(), |v| &v[idx.0])
    }

    /// Push an upvalue at the end of the upvalue list.
    fn push_upvalue(&self, value: Value) {
        self.upvalues_mut().push(value);
    }

    /// Resolve deferred upvalues from the provided stack slice,
    /// mutating them in the internal upvalue slots.
    fn resolve_deferred_upvalues(&self, stack: &[Value]) {
        for upvalue in self.upvalues_mut().iter_mut() {
            if let Value::DeferredUpvalue(idx) = upvalue {
                *upvalue = stack[idx.0].clone();
            }
        }
    }
}
