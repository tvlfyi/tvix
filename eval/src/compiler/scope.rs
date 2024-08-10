//! This module implements the scope-tracking logic of the Tvix
//! compiler.
//!
//! Scoping in Nix is fairly complicated, there are features like
//! mutually recursive bindings, `with`, upvalue capturing, and so
//! on that introduce a fair bit of complexity.
//!
//! Tvix attempts to do as much of the heavy lifting of this at
//! compile time, and leave the runtime to mostly deal with known
//! stack indices. To do this, the compiler simulates where locals
//! will be at runtime using the data structures implemented here.

use rustc_hash::FxHashMap;
use std::{collections::hash_map, ops::Index};

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
    pub span: Option<codemap::Span>,

    /// Scope depth of this local.
    pub depth: usize,

    /// Is this local initialised?
    pub initialised: bool,

    /// Is this local known to have been used at all?
    pub used: bool,

    /// Does this local need to be finalised after the enclosing scope
    /// is completely constructed?
    pub needs_finaliser: bool,

    /// Does this local's upvalues contain a reference to itself?
    pub must_thunk: bool,
}

impl Local {
    /// Retrieve the name of the given local (if available).
    pub fn name(&self) -> Option<SmolStr> {
        match &self.name {
            LocalName::Phantom => None,
            LocalName::Ident(name) => Some(SmolStr::new(name)),
        }
    }

    /// Is this local intentionally ignored? (i.e. name starts with `_`)
    pub fn is_ignored(&self) -> bool {
        match &self.name {
            LocalName::Ident(name) => name.starts_with('_'),
            LocalName::Phantom => false,
        }
    }

    pub fn is_used(&self) -> bool {
        self.depth == 0 || self.used || self.is_ignored()
    }
}

/// Represents the current position of an identifier as resolved in a scope.
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
}

#[derive(Clone, Debug)]
pub struct Upvalue {
    pub kind: UpvalueKind,
}

/// The index of a local in the scope's local array at compile time.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub struct LocalIdx(usize);

/// Helper struct for indexing over `Scope::locals` by name.
#[derive(Debug)]
enum ByName {
    Single(LocalIdx),
    Shadowed(Vec<LocalIdx>),
}

impl ByName {
    /// Add an additional index for this name.
    fn add_idx(&mut self, new: LocalIdx) {
        match self {
            ByName::Shadowed(indices) => indices.push(new),
            ByName::Single(idx) => {
                *self = ByName::Shadowed(vec![*idx, new]);
            }
        }
    }

    /// Remove the most recent index for this name, unless it is a
    /// single. Returns `true` if an entry was removed.
    fn remove_idx(&mut self) -> bool {
        match self {
            ByName::Single(_) => false,
            ByName::Shadowed(indices) => match indices[..] {
                [fst, _snd] => {
                    *self = ByName::Single(fst);
                    true
                }
                _ => {
                    indices.pop();
                    true
                }
            },
        }
    }

    /// Return the most recent index.
    pub fn index(&self) -> LocalIdx {
        match self {
            ByName::Single(idx) => *idx,
            ByName::Shadowed(vec) => *vec.last().unwrap(),
        }
    }
}

/// Represents a scope known during compilation, which can be resolved
/// directly to stack indices.
#[derive(Debug, Default)]
pub struct Scope {
    locals: Vec<Local>,
    pub upvalues: Vec<Upvalue>,

    /// Secondary by-name index over locals.
    by_name: FxHashMap<String, ByName>,

    /// How many scopes "deep" are these locals?
    scope_depth: usize,

    /// Current size of the `with`-stack at runtime.
    with_stack_size: usize,
}

impl Index<LocalIdx> for Scope {
    type Output = Local;

    fn index(&self, index: LocalIdx) -> &Self::Output {
        &self.locals[index.0]
    }
}

impl Scope {
    /// Inherit scope details from a parent scope (required for
    /// correctly nesting scopes in lambdas and thunks when special
    /// scope features like dynamic resolution are present).
    pub fn inherit(&self) -> Self {
        Self {
            scope_depth: self.scope_depth + 1,
            with_stack_size: self.with_stack_size,
            ..Default::default()
        }
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
        if let Some(by_name) = self.by_name.get(name) {
            let idx = by_name.index();
            let local = self
                .locals
                .get_mut(idx.0)
                .expect("invalid compiler state: indexed local missing");

            local.used = true;

            // This local is still being initialised, meaning that
            // we know its final runtime stack position, but it is
            // not yet on the stack.
            if !local.initialised {
                return LocalPosition::Recursive(idx);
            }

            return LocalPosition::Known(idx);
        }

        LocalPosition::Unknown
    }

    /// Declare a local variable that occupies a stack slot and should
    /// be accounted for, but is not directly accessible by users
    /// (e.g. attribute sets used for `with`).
    pub fn declare_phantom(&mut self, span: codemap::Span, initialised: bool) -> LocalIdx {
        let idx = self.locals.len();
        self.locals.push(Local {
            initialised,
            span: Some(span),
            name: LocalName::Phantom,
            depth: self.scope_depth,
            needs_finaliser: false,
            must_thunk: false,
            used: true,
        });

        LocalIdx(idx)
    }

    /// Declare an uninitialised, named local variable.
    ///
    /// Returns the `LocalIdx` of the new local, and optionally the
    /// index of a previous local shadowed by this one.
    pub fn declare_local(
        &mut self,
        name: String,
        span: codemap::Span,
    ) -> (LocalIdx, Option<LocalIdx>) {
        let idx = LocalIdx(self.locals.len());
        self.locals.push(Local {
            name: LocalName::Ident(name.clone()),
            span: Some(span),
            depth: self.scope_depth,
            initialised: false,
            needs_finaliser: false,
            must_thunk: false,
            used: false,
        });

        let mut shadowed = None;
        match self.by_name.entry(name) {
            hash_map::Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                shadowed = Some(existing.index());
                existing.add_idx(idx);
            }
            hash_map::Entry::Vacant(entry) => {
                entry.insert(ByName::Single(idx));
            }
        }

        (idx, shadowed)
    }

    pub fn declare_constant(&mut self, name: String) -> LocalIdx {
        let idx = LocalIdx(self.locals.len());
        self.locals.push(Local {
            name: LocalName::Ident(name.clone()),
            span: None,
            depth: 0,
            initialised: true,
            used: false,
            needs_finaliser: false,
            must_thunk: false,
        });
        // We don't need to worry about shadowing for constants; they're defined at the toplevel
        // always
        self.by_name.insert(name, ByName::Single(idx));
        idx
    }

    /// Mark local as initialised after compiling its expression.
    pub fn mark_initialised(&mut self, idx: LocalIdx) {
        self.locals[idx.0].initialised = true;
    }

    /// Mark local as needing a finaliser.
    pub fn mark_needs_finaliser(&mut self, idx: LocalIdx) {
        self.locals[idx.0].needs_finaliser = true;
    }

    /// Mark local as must be wrapped in a thunk.  This happens if
    /// the local has a reference to itself in its upvalues.
    pub fn mark_must_thunk(&mut self, idx: LocalIdx) {
        self.locals[idx.0].must_thunk = true;
    }

    /// Compute the runtime stack index for a given local by
    /// accounting for uninitialised variables at scopes below this
    /// one.
    pub fn stack_index(&self, idx: LocalIdx) -> StackIdx {
        let uninitialised_count = self.locals[..(idx.0)]
            .iter()
            .filter(|l| !l.initialised && self[idx].depth > l.depth)
            .count();

        StackIdx(idx.0 - uninitialised_count)
    }

    /// Increase the current scope depth (e.g. within a new bindings
    /// block, or `with`-scope).
    pub fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    /// Decrease the scope depth and remove all locals still tracked
    /// for the current scope.
    ///
    /// Returns the count of locals that were dropped while marked as
    /// initialised (used by the compiler to determine whether to emit
    /// scope cleanup operations), as well as the spans of the
    /// definitions of unused locals (used by the compiler to emit
    /// unused binding warnings).
    pub fn end_scope(&mut self) -> (usize, Vec<codemap::Span>) {
        debug_assert!(self.scope_depth != 0, "can not end top scope");

        let mut pops = 0;
        let mut unused_spans = vec![];

        // TL;DR - iterate from the back while things belonging to the
        // ended scope still exist.
        while self.locals.last().unwrap().depth == self.scope_depth {
            if let Some(local) = self.locals.pop() {
                // pop the local from the stack if it was actually
                // initialised
                if local.initialised {
                    pops += 1;
                }

                // analyse whether the local was accessed during its
                // lifetime, and emit a warning otherwise (unless the
                // user explicitly chose to ignore it by prefixing the
                // identifier with `_`)
                if local.is_used() {
                    unused_spans.extend(local.span);
                }

                // remove the by-name index if this was a named local
                if let LocalName::Ident(name) = local.name {
                    if let hash_map::Entry::Occupied(mut entry) = self.by_name.entry(name) {
                        // If no removal occured through `remove_idx`
                        // (i.e. there was no shadowing going on),
                        // nuke the whole entry.
                        if !entry.get_mut().remove_idx() {
                            entry.remove();
                        }
                    }
                }
            }
        }

        self.scope_depth -= 1;

        (pops, unused_spans)
    }

    /// Access the current scope depth.
    pub fn scope_depth(&self) -> usize {
        self.scope_depth
    }
}
