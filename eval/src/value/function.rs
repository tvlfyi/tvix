//! This module implements the runtime representation of functions.
use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};

use crate::{
    chunk::Chunk,
    upvalues::{UpvalueCarrier, Upvalues},
};

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct InnerClosure {
    pub lambda: Rc<Lambda>,
    upvalues: Upvalues,
}

#[repr(transparent)]
#[derive(Clone, Debug, PartialEq)]
pub struct Closure(Rc<RefCell<InnerClosure>>);

impl Closure {
    pub fn new(lambda: Rc<Lambda>) -> Self {
        Closure(Rc::new(RefCell::new(InnerClosure {
            upvalues: Upvalues::with_capacity(lambda.upvalue_count),
            lambda,
        })))
    }

    pub fn chunk(&self) -> Ref<'_, Chunk> {
        Ref::map(self.0.borrow(), |c| &c.lambda.chunk)
    }

    pub fn lambda(&self) -> Rc<Lambda> {
        self.0.borrow().lambda.clone()
    }
}

impl UpvalueCarrier for Closure {
    fn upvalue_count(&self) -> usize {
        self.0.borrow().lambda.upvalue_count
    }

    fn upvalues(&self) -> Ref<'_, Upvalues> {
        Ref::map(self.0.borrow(), |c| &c.upvalues)
    }

    fn upvalues_mut(&self) -> RefMut<'_, Upvalues> {
        RefMut::map(self.0.borrow_mut(), |c| &mut c.upvalues)
    }
}
