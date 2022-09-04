//! This module implements the scope-tracking logic of the Tvix
//! compiler.
//!
//! Scoping in Nix is fairly complicated, there are features like
//! mutually recursive bindings, `with`, upvalue capturing, builtin
//! poisoning and so on that introduce a fair bit of complexity.
//!
//! Tvix attempts to do as much of the heavy lifting of this at
//! compile time, and leave the runtime to mostly deal with known
//! stack indices. To do this, the compiler simulates where locals
//! will be at runtime using the data structures implemented here.

use std::{
    collections::{hash_map, HashMap},
    ops::Index,
};

use smol_str::SmolStr;

use crate::opcode::{StackIdx, UpvalueIdx};

#[derive(Debug)]
enum LocalName {
    /// Normally declared local with a statically known name.
    Ident(String),

    /// Phantom stack value (e.g. attribute set used for `with`) that
    /// must be accounted for to calculate correct stack offsets.
    Phantom,
}

/// Represents a single local already known to the compiler.
#[derive(Debug)]
pub struct Local {
    /// Identifier of this local. This is always a statically known
    /// value (Nix does not allow dynamic identifier names in locals),
    /// or a "phantom" value not accessible by users.
    name: LocalName,

    /// Source span at which this local was declared.
    pub span: codemap::Span,

    /// Scope depth of this local.
    pub depth: usize,

    /// Is this local initialised?
    pub initialised: bool,

    /// Is this local known to have been used at all?
    pub used: bool,

    /// Does this local need to be finalised after the enclosing scope
    /// is completely constructed?
    pub needs_finaliser: bool,
}

impl Local {
    /// Does this local live above the other given depth?
    pub fn above(&self, theirs: usize) -> bool {
        self.depth > theirs
    }

    /// Does the name of this local match the given string?
    pub fn has_name(&self, other: &str) -> bool {
        match &self.name {
            LocalName::Ident(name) => name == other,

            // Phantoms are *never* accessible by a name.
            LocalName::Phantom => false,
        }
    }

    /// Is this local intentionally ignored? (i.e. name starts with `_`)
    pub fn is_ignored(&self) -> bool {
        match &self.name {
            LocalName::Ident(name) => name.starts_with('_'),
            LocalName::Phantom => false,
        }
    }
}

/// Represents the current position of a local as resolved in a scope.
pub enum LocalPosition {
    /// Local is not known in this scope.
    Unknown,

    /// Local is known at the given local index.
    Known(LocalIdx),

    /// Local is known, but is being accessed recursively within its
    /// own initialisation. Depending on context, this is either an
    /// error or forcing a closure/thunk.
    Recursive(LocalIdx),
}

/// Represents the different ways in which upvalues can be captured in
/// closures or thunks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpvalueKind {
    /// This upvalue captures a local from the stack.
    Local(LocalIdx),

    /// This upvalue captures an enclosing upvalue.
    Upvalue(UpvalueIdx),

    /// This upvalue captures a dynamically resolved value (i.e.
    /// `with`).
    ///
    /// It stores the identifier with which to perform a dynamic
    /// lookup, as well as the optional upvalue index in the enclosing
    /// function (if any).
    Dynamic {
        name: SmolStr,
        up: Option<UpvalueIdx>,
    },
}

#[derive(Clone, Debug)]
pub struct Upvalue {
    pub kind: UpvalueKind,
    pub node: rnix::ast::Ident,
}

/// Represents the index of a local in the scope's local array, which
/// is subtly different from its `StackIdx` (which excludes
/// uninitialised values in between).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub struct LocalIdx(usize);

/// Represents a scope known during compilation, which can be resolved
/// directly to stack indices.
#[derive(Debug, Default)]
pub struct Scope {
    pub locals: Vec<Local>,
    pub upvalues: Vec<Upvalue>,

    /// How many scopes "deep" are these locals?
    pub scope_depth: usize,

    /// Current size of the `with`-stack at runtime.
    with_stack_size: usize,

    /// Users are allowed to override globally defined symbols like
    /// `true`, `false` or `null` in scopes. We call this "scope
    /// poisoning", as it requires runtime resolution of those tokens.
    ///
    /// To support this efficiently, the depth at which a poisoning
    /// occured is tracked here.
    poisoned_tokens: HashMap<&'static str, usize>,
}

impl Index<LocalIdx> for Scope {
    type Output = Local;

    fn index(&self, index: LocalIdx) -> &Self::Output {
        &self.locals[index.0]
    }
}

impl Scope {
    /// Mark a globally defined token as poisoned.
    pub fn poison(&mut self, name: &'static str, depth: usize) {
        match self.poisoned_tokens.entry(name) {
            hash_map::Entry::Occupied(_) => {
                /* do nothing, as the token is already poisoned at a
                 * lower scope depth */
            }
            hash_map::Entry::Vacant(entry) => {
                entry.insert(depth);
            }
        }
    }

    /// Inherit scope details from a parent scope (required for
    /// correctly nesting scopes in lambdas and thunks when special
    /// scope features like poisoning are present).
    pub fn inherit(&self) -> Self {
        Self {
            poisoned_tokens: self.poisoned_tokens.clone(),
            scope_depth: self.scope_depth,
            ..Default::default()
        }
    }

    /// Check whether a given token is poisoned.
    pub fn is_poisoned(&self, name: &str) -> bool {
        self.poisoned_tokens.contains_key(name)
    }

    /// "Unpoison" tokens that were poisoned at a given depth. Used
    /// when scopes are closed.
    pub fn unpoison(&mut self, depth: usize) {
        self.poisoned_tokens
            .retain(|_, poisoned_at| *poisoned_at != depth);
    }

    /// Increase the `with`-stack size of this scope.
    pub fn push_with(&mut self) {
        self.with_stack_size += 1;
    }

    /// Decrease the `with`-stack size of this scope.
    pub fn pop_with(&mut self) {
        self.with_stack_size -= 1;
    }

    /// Does this scope currently require dynamic runtime resolution
    /// of identifiers that could not be found?
    pub fn has_with(&self) -> bool {
        self.with_stack_size > 0
    }

    /// Resolve the stack index of a statically known local.
    pub fn resolve_local(&mut self, name: &str) -> LocalPosition {
        for (idx, local) in self.locals.iter_mut().enumerate().rev() {
            if local.has_name(name) {
                local.used = true;

                // This local is still being initialised, meaning that
                // we know its final runtime stack position, but it is
                // not yet on the stack.
                if !local.initialised {
                    return LocalPosition::Recursive(LocalIdx(idx));
                }

                return LocalPosition::Known(LocalIdx(idx));
            }
        }

        LocalPosition::Unknown
    }

    /// Declare a local variable that occupies a stack slot and should
    /// be accounted for, but is not directly accessible by users
    /// (e.g. attribute sets used for `with`).
    pub fn declare_phantom(&mut self, span: codemap::Span) -> LocalIdx {
        let idx = self.locals.len();
        self.locals.push(Local {
            name: LocalName::Phantom,
            span,
            depth: self.scope_depth,
            initialised: false,
            needs_finaliser: false,
            used: true,
        });

        LocalIdx(idx)
    }

    /// Declare an uninitialised local variable.
    pub fn declare_local(&mut self, name: String, span: codemap::Span) -> LocalIdx {
        let idx = self.locals.len();
        self.locals.push(Local {
            name: LocalName::Ident(name),
            span,
            depth: self.scope_depth,
            initialised: false,
            needs_finaliser: false,
            used: false,
        });

        LocalIdx(idx)
    }

    /// Mark local as initialised after compiling its expression.
    pub fn mark_initialised(&mut self, idx: LocalIdx) {
        self.locals[idx.0].initialised = true;
    }

    /// Mark local as needing a finaliser.
    pub fn mark_needs_finaliser(&mut self, idx: LocalIdx) {
        self.locals[idx.0].needs_finaliser = true;
    }

    /// Compute the runtime stack index for a given local by
    /// accounting for uninitialised variables at scopes below this
    /// one.
    pub fn stack_index(&self, idx: LocalIdx) -> StackIdx {
        let uninitialised_count = self.locals[..(idx.0)]
            .iter()
            .filter(|l| !l.initialised && self[idx].above(l.depth))
            .count();

        StackIdx(idx.0 - uninitialised_count)
    }
}
