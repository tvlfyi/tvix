//! This module encapsulates some logic for upvalue handling, which is
//! relevant to both thunks (delayed computations for lazy-evaluation)
//! as well as closures (lambdas that capture variables from the
//! surrounding scope).
//!
//! The upvalues of a scope are whatever data are needed at runtime
//! in order to resolve each free variable in the scope to a value.
//! "Upvalue" is a term taken from Lua.

use std::{
    cell::{Ref, RefMut},
    ops::Index,
};

use crate::{opcode::UpvalueIdx, Value};

/// Structure for carrying upvalues of an UpvalueCarrier.  The
/// implementation of this struct encapsulates the logic for
/// capturing and accessing upvalues.
///
/// Nix's `with` cannot be used to shadow an enclosing binding --
/// like Rust's `use xyz::*` construct, but unlike Javascript's
/// `with (xyz)`.  This means that Nix has two kinds of identifiers,
/// which can be distinguished at compile time:
///
/// - Static identifiers, which are bound in some enclosing scope by
///   `let`, `name:` or `{name}:`
/// - Dynamic identifiers, which are not bound in any enclosing
///   scope
#[derive(Clone, Debug, PartialEq)]
pub struct Upvalues {
    /// The upvalues of static identifiers.  Each static identifier
    /// is assigned an integer identifier at compile time, which is
    /// an index into this Vec.
    static_upvalues: Vec<Value>,

    /// The upvalues of dynamic identifiers, if any exist.  This
    /// consists of the value passed to each enclosing `with val;`,
    /// from outermost to innermost.
    with_stack: Option<Vec<Value>>,
}

impl Upvalues {
    pub fn with_capacity(count: usize) -> Self {
        Upvalues {
            static_upvalues: Vec::with_capacity(count),
            with_stack: None,
        }
    }

    /// Push an upvalue at the end of the upvalue list.
    pub fn push(&mut self, value: Value) {
        self.static_upvalues.push(value);
    }

    /// Set the captured with stack.
    pub fn set_with_stack(&mut self, with_stack: Vec<Value>) {
        self.with_stack = Some(with_stack);
    }

    pub fn with_stack(&self) -> Option<&Vec<Value>> {
        self.with_stack.as_ref()
    }

    pub fn with_stack_len(&self) -> usize {
        match &self.with_stack {
            None => 0,
            Some(stack) => stack.len(),
        }
    }
}

impl Index<UpvalueIdx> for Upvalues {
    type Output = Value;

    fn index(&self, index: UpvalueIdx) -> &Self::Output {
        &self.static_upvalues[index.0]
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
        Ref::map(self.upvalues(), |v| &v.static_upvalues[idx.0])
    }

    /// Resolve deferred upvalues from the provided stack slice,
    /// mutating them in the internal upvalue slots.
    fn resolve_deferred_upvalues(&self, stack: &[Value]) {
        for upvalue in self.upvalues_mut().static_upvalues.iter_mut() {
            if let Value::DeferredUpvalue(idx) = upvalue {
                *upvalue = stack[idx.0].clone();
            }
        }
    }
}
