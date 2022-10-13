//! This module implements the runtime representation of functions.
use std::{
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    hash::Hash,
    rc::Rc,
};

use codemap::Span;

use crate::{
    chunk::Chunk,
    upvalues::{UpvalueCarrier, Upvalues},
};

use super::NixString;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Formals {
    /// Map from argument name, to whether that argument is required
    pub(crate) arguments: HashMap<NixString, bool>,

    /// Do the formals of this function accept extra arguments
    pub(crate) ellipsis: bool,

    /// The span of the formals themselves, to use to emit errors
    pub(crate) span: Span,
}

impl Formals {
    /// Returns true if the given arg is a valid argument to these formals.
    ///
    /// This is true if it is either listed in the list of arguments, or the formals have an
    /// ellipsis
    pub(crate) fn contains<Q>(&self, arg: &Q) -> bool
    where
        Q: ?Sized + Hash + Eq,
        NixString: std::borrow::Borrow<Q>,
    {
        self.ellipsis || self.arguments.contains_key(&arg)
    }
}

/// The opcodes for a thunk or closure, plus the number of
/// non-executable opcodes which are allowed after an OpClosure or
/// OpThunk referencing it.  At runtime `Lambda` is usually wrapped
/// in `Rc` to avoid copying the `Chunk` it holds (which can be
/// quite large).
#[derive(Debug, PartialEq)]
pub struct Lambda {
    pub(crate) chunk: Chunk,

    /// Number of upvalues which the code in this Lambda closes
    /// over, and which need to be initialised at
    /// runtime.  Information about the variables is emitted using
    /// data-carrying opcodes (see [`OpCode::DataLocalIdx`]).
    pub(crate) upvalue_count: usize,
    pub(crate) formals: Option<Formals>,
}

impl Lambda {
    pub fn new_anonymous() -> Self {
        Lambda {
            chunk: Default::default(),
            upvalue_count: 0,
            formals: None,
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
