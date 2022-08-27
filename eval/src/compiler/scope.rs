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

use std::collections::{hash_map, HashMap};

use smol_str::SmolStr;

use crate::opcode::{StackIdx, UpvalueIdx};

/// Represents the initialisation status of a variable, tracking
/// whether it is only known or also already defined.
pub enum Depth {
    /// Variable is defined and located at the given depth.
    At(usize),

    /// Variable is known but not yet defined.
    Unitialised,
}

impl Depth {
    /// Does this variable live above the other given depth?
    pub fn above(&self, theirs: usize) -> bool {
        match self {
            Depth::Unitialised => false,
            Depth::At(ours) => *ours > theirs,
        }
    }

    /// Does this variable live below the other given depth?
    pub fn below(&self, theirs: usize) -> bool {
        match self {
            Depth::Unitialised => false,
            Depth::At(ours) => *ours < theirs,
        }
    }
}

/// Represents a single local already known to the compiler.
pub struct Local {
    // Definition name, which can be different kinds of tokens (plain
    // string or identifier). Nix does not allow dynamic names inside
    // of `let`-expressions.
    pub name: String,

    // Syntax node at which this local was declared.
    pub node: Option<rnix::SyntaxNode>,

    // Scope depth of this local.
    pub depth: Depth,

    // Phantom locals are not actually accessible by users (e.g.
    // intermediate values used for `with`).
    pub phantom: bool,

    // Is this local known to have been used at all?
    pub used: bool,
}

/// Represents the current position of a local as resolved in a scope.
pub enum LocalPosition {
    /// Local is not known in this scope.
    Unknown,

    /// Local is known and defined at the given stack index.
    Known(StackIdx),

    /// Local is known, but is being accessed recursively within its
    /// own initialisation. Depending on context, this is either an
    /// error or forcing a closure/thunk.
    Recursive(StackIdx),
}

/// Represents the different ways in which upvalues can be captured in
/// closures or thunks.
#[derive(Debug, PartialEq)]
pub enum Upvalue {
    /// This upvalue captures a local from the stack.
    Stack(StackIdx),

    /// This upvalue captures an enclosing upvalue.
    Upvalue(UpvalueIdx),

    /// This upvalue captures a dynamically resolved value (i.e.
    /// `with`).
    Dynamic(SmolStr),
}

/// Represents a scope known during compilation, which can be resolved
/// directly to stack indices.
///
/// TODO(tazjin): `with`-stack
/// TODO(tazjin): flag "specials" (e.g. note depth if builtins are
/// overridden)
#[derive(Default)]
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

                match local.depth {
                    // This local is still being initialised, meaning
                    // that we know its final runtime stack position,
                    // but it is not yet on the stack.
                    Depth::Unitialised => return LocalPosition::Recursive(StackIdx(idx)),

                    // This local is known, but we need to account for
                    // uninitialised variables in this "initialiser
                    // stack".
                    Depth::At(_) => return LocalPosition::Known(self.resolve_uninit(idx)),
                }
            }
        }

        LocalPosition::Unknown
    }

    /// Return the "initialiser stack slot" of a value, that is the
    /// stack slot of a value which might only exist during the
    /// initialisation of another. This requires accounting for the
    /// stack offsets of any unitialised variables.
    fn resolve_uninit(&mut self, locals_idx: usize) -> StackIdx {
        StackIdx(
            self.locals[..locals_idx]
                .iter()
                .filter(|local| matches!(local.depth, Depth::At(_)))
                .count(),
        )
    }
}
