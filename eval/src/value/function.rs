//! This module implements the runtime representation of functions.
use std::{
    cell::{Ref, RefCell},
    rc::Rc,
};

use crate::{chunk::Chunk, opcode::UpvalueIdx, Value};

#[derive(Clone, Debug)]
pub struct Lambda {
    // name: Option<NixString>,
    pub(crate) chunk: Chunk,
    pub(crate) upvalue_count: usize,
}

impl Lambda {
    pub fn new_anonymous() -> Self {
        Lambda {
            // name: None,
            chunk: Default::default(),
            upvalue_count: 0,
        }
    }

    pub fn chunk(&mut self) -> &mut Chunk {
        &mut self.chunk
    }
}

#[derive(Clone, Debug)]
pub struct InnerClosure {
    pub lambda: Rc<Lambda>,
    pub upvalues: Vec<Value>,
}

#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct Closure(Rc<RefCell<InnerClosure>>);

impl Closure {
    pub fn new(lambda: Rc<Lambda>) -> Self {
        Closure(Rc::new(RefCell::new(InnerClosure {
            upvalues: Vec::with_capacity(lambda.upvalue_count),
            lambda,
        })))
    }

    pub fn chunk(&self) -> Ref<'_, Chunk> {
        Ref::map(self.0.borrow(), |c| &c.lambda.chunk)
    }

    pub fn upvalue(&self, idx: UpvalueIdx) -> Ref<'_, Value> {
        Ref::map(self.0.borrow(), |c| &c.upvalues[idx.0])
    }

    pub fn upvalue_count(&self) -> usize {
        self.0.borrow().lambda.upvalue_count
    }

    pub fn push_upvalue(&self, value: Value) {
        self.0.borrow_mut().upvalues.push(value)
    }

    /// Resolve the deferred upvalues in the closure from a slice of
    /// the current stack, using the indices stored in the deferred
    /// values.
    pub fn resolve_deferred_upvalues(&self, stack: &[Value]) {
        for upvalue in self.0.borrow_mut().upvalues.iter_mut() {
            if let Value::DeferredUpvalue(idx) = upvalue {
                *upvalue = stack[idx.0].clone();
            }
        }
    }
}
