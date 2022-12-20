//! This module implements the runtime representation of functions.
use std::{collections::HashMap, hash::Hash, rc::Rc};

use codemap::Span;
use smol_str::SmolStr;

use crate::{chunk::Chunk, upvalues::Upvalues};

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
        self.ellipsis || self.arguments.contains_key(arg)
    }
}

/// The opcodes for a thunk or closure, plus the number of
/// non-executable opcodes which are allowed after an OpThunkClosure or
/// OpThunkSuspended referencing it.  At runtime `Lambda` is usually wrapped
/// in `Rc` to avoid copying the `Chunk` it holds (which can be
/// quite large).
///
/// In order to correctly reproduce cppnix's "pointer equality"
/// semantics it is important that we never clone a Lambda --
/// use Rc<Lambda>::clone() instead.  This struct deliberately
/// does not `derive(Clone)` in order to prevent this from being
/// done accidentally.
///
#[derive(/* do not add Clone here */ Debug, Default)]
pub struct Lambda {
    pub(crate) chunk: Chunk,

    /// Name of the function (equivalent to the name of the
    /// identifier (e.g. a value in a let-expression or an attribute
    /// set entry) it is located in).
    pub(crate) name: Option<SmolStr>,

    /// Number of upvalues which the code in this Lambda closes
    /// over, and which need to be initialised at
    /// runtime.  Information about the variables is emitted using
    /// data-carrying opcodes (see [`OpCode::DataStackIdx`]).
    pub(crate) upvalue_count: usize,
    pub(crate) formals: Option<Formals>,
}

impl Lambda {
    pub fn chunk(&mut self) -> &mut Chunk {
        &mut self.chunk
    }
}

///
/// In order to correctly reproduce cppnix's "pointer equality"
/// semantics it is important that we never clone a Lambda --
/// use Rc<Lambda>::clone() instead.  This struct deliberately
/// does not `derive(Clone)` in order to prevent this from being
/// done accidentally.
///
#[derive(/* do not add Clone here */ Debug)]
pub struct Closure {
    pub lambda: Rc<Lambda>,
    pub upvalues: Rc<Upvalues>,
}

impl Closure {
    pub fn new(lambda: Rc<Lambda>) -> Self {
        Self::new_with_upvalues(
            Rc::new(Upvalues::with_capacity(lambda.upvalue_count)),
            lambda,
        )
    }

    pub fn new_with_upvalues(upvalues: Rc<Upvalues>, lambda: Rc<Lambda>) -> Self {
        Closure { upvalues, lambda }
    }

    pub fn chunk(&self) -> &Chunk {
        &self.lambda.chunk
    }

    pub fn lambda(&self) -> Rc<Lambda> {
        self.lambda.clone()
    }

    pub fn upvalues(&self) -> Rc<Upvalues> {
        self.upvalues.clone()
    }
}
