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

/// Represents a single local already known to the compiler.
#[derive(Debug)]
pub struct Local {
    // Definition name, which can be different kinds of tokens (plain
    // string or identifier). Nix does not allow dynamic names inside
    // of `let`-expressions.
    pub name: String,

    // Syntax node at which this local was declared.
    pub node: Option<rnix::SyntaxNode>,

    // Scope depth of this local.
    pub depth: usize,

    // Is this local initialised?
    pub initialised: bool,

    // Phantom locals are not actually accessible by users (e.g.
    // intermediate values used for `with`).
    pub phantom: bool,

    // Is this local known to have been used at all?
    pub used: bool,

    // Does this local need to be finalised after the enclosing scope
    // is completely constructed?
    pub needs_finaliser: bool,
}

impl Local {
    /// Does this local live above the other given depth?
    pub fn above(&self, theirs: usize) -> bool {
        self.depth > theirs
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
}

/// Represents the index of a local in the scope's local array, which
/// is subtly different from its `StackIdx` (which excludes
/// uninitialised values in between).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub struct LocalIdx(usize);

/// Represents a scope known during compilation, which can be resolved
/// directly to stack indices.
///
/// TODO(tazjin): `with`-stack
/// TODO(tazjin): flag "specials" (e.g. note depth if builtins are
/// overridden)
#[derive(Debug, Default)]
pub struct Scope {
    pub locals: Vec<Local>,
    pub upvalues: Vec<Upvalue>,

    // How many scopes "deep" are these locals?
    pub scope_depth: usize,

    // Current size of the `with`-stack at runtime.
    with_stack_size: usize,

    // Users are allowed to override globally defined symbols like
    // `true`, `false` or `null` in scopes. We call this "scope
    // poisoning", as it requires runtime resolution of those tokens.
    //
    // To support this efficiently, the depth at which a poisoning
    // occured is tracked here.
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
            if !local.phantom && local.name == name {
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
    pub fn declare_phantom(&mut self) -> LocalIdx {
        let idx = self.locals.len();
        self.locals.push(Local {
            depth: self.scope_depth,
            initialised: true,
            needs_finaliser: false,
            name: "".into(),
            node: None,
            phantom: true,
            used: true,
        });

        LocalIdx(idx)
    }

    /// Declare an uninitialised local variable.
    pub fn declare_local(&mut self, name: String, node: rnix::SyntaxNode) -> LocalIdx {
        let idx = self.locals.len();
        self.locals.push(Local {
            name,
            depth: self.scope_depth,
            initialised: false,
            needs_finaliser: false,
            node: Some(node),
            phantom: false,
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
