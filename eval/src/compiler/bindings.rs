//! This module implements compiler logic related to name/value binding
//! definitions (that is, attribute sets and let-expressions).
//!
//! In the case of recursive scopes these cases share almost all of their
//! (fairly complex) logic.

use std::iter::Peekable;

use rnix::ast::HasEntry;
use rowan::ast::AstChildren;

use super::*;

type PeekableAttrs = Peekable<AstChildren<ast::Attr>>;

/// What kind of bindings scope is being compiled?
#[derive(Clone, Copy, PartialEq)]
enum BindingsKind {
    /// Standard `let ... in ...`-expression.
    LetIn,

    /// Non-recursive attribute set.
    Attrs,

    /// Recursive attribute set.
    RecAttrs,
}

impl BindingsKind {
    fn is_attrs(&self) -> bool {
        matches!(self, BindingsKind::Attrs | BindingsKind::RecAttrs)
    }
}

// Internal representation of an attribute set used for merging sets, or
// inserting nested keys.
#[derive(Clone)]
struct AttributeSet {
    /// Original span at which this set was first encountered.
    span: Span,

    /// Tracks the kind of set (rec or not).
    kind: BindingsKind,

    /// All inherited entries
    inherits: Vec<ast::Inherit>,

    /// All internal entries
    entries: Vec<(Span, PeekableAttrs, ast::Expr)>,
}

impl ToSpan for AttributeSet {
    fn span_for(&self, _: &codemap::File) -> Span {
        self.span
    }
}

impl AttributeSet {
    fn from_ast(c: &Compiler, node: &ast::AttrSet) -> Self {
        AttributeSet {
            span: c.span_for(node),

            // Kind of the attrs depends on the first time it is
            // encountered. We actually believe this to be a Nix
            // bug: https://github.com/NixOS/nix/issues/7111
            kind: if node.rec_token().is_some() {
                BindingsKind::RecAttrs
            } else {
                BindingsKind::Attrs
            },

            inherits: ast::HasEntry::inherits(node).collect(),

            entries: ast::HasEntry::attrpath_values(node)
                .map(|entry| {
                    let span = c.span_for(&entry);
                    (
                        span,
                        entry.attrpath().unwrap().attrs().peekable(),
                        entry.value().unwrap(),
                    )
                })
                .collect(),
        }
    }
}

// Data structures to track the bindings observed in the second pass, and
// forward the information needed to compile their value.
enum Binding {
    InheritFrom {
        namespace: ast::Expr,
        name: SmolStr,
        span: Span,
    },

    Plain {
        expr: ast::Expr,
    },

    Set(AttributeSet),
}

impl Binding {
    /// Merge the provided value into the current binding, or emit an
    /// error if this turns out to be impossible.
    fn merge(
        &mut self,
        c: &mut Compiler,
        span: Span,
        mut remaining_path: PeekableAttrs,
        value: ast::Expr,
    ) {
        match self {
            Binding::InheritFrom { name, ref span, .. } => {
                c.emit_error(span, ErrorKind::UnmergeableInherit { name: name.clone() })
            }

            // If the value is not yet a nested binding, flip the representation
            // and recurse.
            Binding::Plain { expr } => match expr {
                ast::Expr::AttrSet(existing) => {
                    let nested = AttributeSet::from_ast(c, existing);
                    *self = Binding::Set(nested);
                    self.merge(c, span, remaining_path, value);
                }

                _ => c.emit_error(&value, ErrorKind::UnmergeableValue),
            },

            // If the value is nested further, it is simply inserted into the
            // bindings with its full path and resolved recursively further
            // down.
            Binding::Set(existing) if remaining_path.peek().is_some() => {
                existing.entries.push((span, remaining_path, value))
            }

            Binding::Set(existing) => {
                if let ast::Expr::AttrSet(new) = value {
                    existing.inherits.extend(ast::HasEntry::inherits(&new));
                    existing
                        .entries
                        .extend(ast::HasEntry::attrpath_values(&new).map(|entry| {
                            let span = c.span_for(&entry);
                            (
                                span,
                                entry.attrpath().unwrap().attrs().peekable(),
                                entry.value().unwrap(),
                            )
                        }));
                } else {
                    // This branch is unreachable because in cases where the
                    // path is empty (i.e. there is no further nesting), the
                    // previous try_merge function already verified that the
                    // expression is an attribute set.

                    // TODO(tazjin): Consider making this branch live by
                    // shuffling that check around and emitting a static error
                    // here instead of a runtime error.
                    unreachable!()
                }
            }
        }
    }
}

enum KeySlot {
    /// There is no key slot (`let`-expressions do not emit their key).
    None { name: SmolStr },

    /// The key is statically known and has a slot.
    Static { slot: LocalIdx, name: SmolStr },

    /// The key is dynamic, i.e. only known at runtime, and must be compiled
    /// into its slot.
    Dynamic { slot: LocalIdx, attr: ast::Attr },
}

struct TrackedBinding {
    key_slot: KeySlot,
    value_slot: LocalIdx,
    binding: Binding,
}

impl TrackedBinding {
    /// Does this binding match the given key?
    ///
    /// Used to determine which binding to merge another one into.
    fn matches(&self, key: &str) -> bool {
        match &self.key_slot {
            KeySlot::None { name } => name == key,
            KeySlot::Static { name, .. } => name == key,
            KeySlot::Dynamic { .. } => false,
        }
    }
}

struct TrackedBindings {
    bindings: Vec<TrackedBinding>,
}

impl TrackedBindings {
    fn new() -> Self {
        TrackedBindings { bindings: vec![] }
    }

    /// Attempt to merge an entry into an existing matching binding, assuming
    /// that the provided binding is mergable (i.e. either a nested key or an
    /// attribute set literal).
    ///
    /// Returns true if the binding was merged, false if it needs to be compiled
    /// separately as a new binding.
    fn try_merge(
        &mut self,
        c: &mut Compiler,
        span: Span,
        name: &ast::Attr,
        mut remaining_path: PeekableAttrs,
        value: ast::Expr,
    ) -> bool {
        // If the path has no more entries, and if the entry is not an
        // attribute set literal, the entry can not be merged.
        if remaining_path.peek().is_none() && !matches!(value, ast::Expr::AttrSet(_)) {
            return false;
        }

        // If the first element of the path is not statically known, the entry
        // can not be merged.
        let name = match c.expr_static_attr_str(name) {
            Some(name) => name,
            None => return false,
        };

        // If there is no existing binding with this key, the entry can not be
        // merged.
        // TODO: benchmark whether using a map or something is useful over the
        // `find` here
        let binding = match self.bindings.iter_mut().find(|b| b.matches(&name)) {
            Some(b) => b,
            None => return false,
        };

        // No more excuses ... the binding can be merged!
        binding.binding.merge(c, span, remaining_path, value);

        true
    }

    /// Add a completely new binding to the tracked bindings.
    fn track_new(&mut self, key_slot: KeySlot, value_slot: LocalIdx, binding: Binding) {
        self.bindings.push(TrackedBinding {
            key_slot,
            value_slot,
            binding,
        });
    }
}

/// Wrapper around the `ast::HasEntry` trait as that trait can not be
/// implemented for custom types.
trait HasEntryProxy {
    fn inherits(&self) -> Box<dyn Iterator<Item = ast::Inherit>>;

    fn attributes(
        &self,
        file: Arc<codemap::File>,
    ) -> Box<dyn Iterator<Item = (Span, PeekableAttrs, ast::Expr)>>;
}

impl<N: HasEntry> HasEntryProxy for N {
    fn inherits(&self) -> Box<dyn Iterator<Item = ast::Inherit>> {
        Box::new(ast::HasEntry::inherits(self))
    }

    fn attributes(
        &self,
        file: Arc<codemap::File>,
    ) -> Box<dyn Iterator<Item = (Span, PeekableAttrs, ast::Expr)>> {
        Box::new(ast::HasEntry::attrpath_values(self).map(move |entry| {
            (
                entry.span_for(&file),
                entry.attrpath().unwrap().attrs().peekable(),
                entry.value().unwrap(),
            )
        }))
    }
}

impl HasEntryProxy for AttributeSet {
    fn inherits(&self) -> Box<dyn Iterator<Item = ast::Inherit>> {
        Box::new(self.inherits.clone().into_iter())
    }

    fn attributes(
        &self,
        _: Arc<codemap::File>,
    ) -> Box<dyn Iterator<Item = (Span, PeekableAttrs, ast::Expr)>> {
        Box::new(self.entries.clone().into_iter())
    }
}

/// AST-traversing functions related to bindings.
impl Compiler<'_> {
    /// Compile all inherits of a node with entries that do *not* have a
    /// namespace to inherit from, and return the remaining ones that do.
    fn compile_plain_inherits<N>(
        &mut self,
        slot: LocalIdx,
        kind: BindingsKind,
        count: &mut usize,
        node: &N,
    ) -> Vec<(ast::Expr, SmolStr, Span)>
    where
        N: ToSpan + HasEntryProxy,
    {
        // Pass over all inherits, resolving only those without namespaces.
        // Since they always resolve in a higher scope, we can just compile and
        // declare them immediately.
        //
        // Inherits with namespaces are returned to the caller.
        let mut inherit_froms: Vec<(ast::Expr, SmolStr, Span)> = vec![];

        for inherit in node.inherits() {
            match inherit.from() {
                // Within a `let` binding, inheriting from the outer scope is a
                // no-op *if* there are no dynamic bindings.
                None if !kind.is_attrs() && !self.has_dynamic_ancestor() => {
                    self.emit_warning(&inherit, WarningKind::UselessInherit);
                    continue;
                }

                None => {
                    for attr in inherit.attrs() {
                        let name = match self.expr_static_attr_str(&attr) {
                            Some(name) => name,
                            None => {
                                self.emit_error(&attr, ErrorKind::DynamicKeyInScope("inherit"));
                                continue;
                            }
                        };

                        // If the identifier resolves statically in a `let`, it
                        // has precedence over dynamic bindings, and the inherit
                        // is useless.
                        if kind == BindingsKind::LetIn
                            && matches!(
                                self.scope_mut().resolve_local(&name),
                                LocalPosition::Known(_)
                            )
                        {
                            self.emit_warning(&attr, WarningKind::UselessInherit);
                            continue;
                        }

                        *count += 1;

                        // Place key on the stack when compiling attribute sets.
                        if kind.is_attrs() {
                            self.emit_constant(Value::String(name.clone().into()), &attr);
                            let span = self.span_for(&attr);
                            self.scope_mut().declare_phantom(span, true);
                        }

                        // Place the value on the stack. Note that because plain
                        // inherits are always in the outer scope, the slot of
                        // *this* scope itself is used.
                        self.compile_identifier_access(slot, &name, &attr);

                        // In non-recursive attribute sets, the key slot must be
                        // a phantom (i.e. the identifier can not be resolved in
                        // this scope).
                        let idx = if kind == BindingsKind::Attrs {
                            let span = self.span_for(&attr);
                            self.scope_mut().declare_phantom(span, false)
                        } else {
                            self.declare_local(&attr, name)
                        };

                        self.scope_mut().mark_initialised(idx);
                    }
                }

                Some(from) => {
                    for attr in inherit.attrs() {
                        let name = match self.expr_static_attr_str(&attr) {
                            Some(name) => name,
                            None => {
                                self.emit_error(&attr, ErrorKind::DynamicKeyInScope("inherit"));
                                continue;
                            }
                        };

                        *count += 1;
                        inherit_froms.push((from.expr().unwrap(), name, self.span_for(&attr)));
                    }
                }
            }
        }

        inherit_froms
    }

    /// Declare all namespaced inherits, that is inherits which are inheriting
    /// values from an attribute set.
    ///
    /// This only ensures that the locals stack is aware of the inherits, it
    /// does not yet emit bytecode that places them on the stack. This is up to
    /// the owner of the `bindings` vector, which this function will populate.
    fn declare_namespaced_inherits(
        &mut self,
        kind: BindingsKind,
        inherit_froms: Vec<(ast::Expr, SmolStr, Span)>,
        bindings: &mut TrackedBindings,
    ) {
        for (from, name, span) in inherit_froms {
            let key_slot = if kind.is_attrs() {
                // In an attribute set, the keys themselves are placed on the
                // stack but their stack slot is inaccessible (it is only
                // consumed by `OpAttrs`).
                KeySlot::Static {
                    slot: self.scope_mut().declare_phantom(span, false),
                    name: name.clone(),
                }
            } else {
                KeySlot::None { name: name.clone() }
            };

            let value_slot = match kind {
                // In recursive scopes, the value needs to be accessible on the
                // stack.
                BindingsKind::LetIn | BindingsKind::RecAttrs => {
                    self.declare_local(&span, name.clone())
                }

                // In non-recursive attribute sets, the value is inaccessible
                // (only consumed by `OpAttrs`).
                BindingsKind::Attrs => self.scope_mut().declare_phantom(span, false),
            };

            bindings.track_new(
                key_slot,
                value_slot,
                Binding::InheritFrom {
                    namespace: from,
                    name,
                    span,
                },
            );
        }
    }

    /// Declare all regular bindings (i.e. `key = value;`) in a bindings scope,
    /// but do not yet compile their values.
    fn declare_bindings<N>(
        &mut self,
        kind: BindingsKind,
        count: &mut usize,
        bindings: &mut TrackedBindings,
        node: &N,
    ) where
        N: ToSpan + HasEntryProxy,
    {
        for (span, mut path, value) in node.attributes(self.file.clone()) {
            let key = path.next().unwrap();

            if bindings.try_merge(self, span, &key, path.clone(), value.clone()) {
                // Binding is nested, or already exists and was merged, move on.
                continue;
            }

            *count += 1;

            let key_span = self.span_for(&key);
            let key_slot = match self.expr_static_attr_str(&key) {
                Some(name) if kind.is_attrs() => KeySlot::Static {
                    name,
                    slot: self.scope_mut().declare_phantom(key_span, false),
                },

                Some(name) => KeySlot::None { name },

                None if kind.is_attrs() => KeySlot::Dynamic {
                    attr: key,
                    slot: self.scope_mut().declare_phantom(key_span, false),
                },

                None => {
                    self.emit_error(&key, ErrorKind::DynamicKeyInScope("let-expression"));
                    continue;
                }
            };

            let value_slot = match kind {
                BindingsKind::LetIn | BindingsKind::RecAttrs => match &key_slot {
                    // In recursive scopes, the value needs to be accessible on the
                    // stack if it is statically known
                    KeySlot::None { name } | KeySlot::Static { name, .. } => {
                        self.declare_local(&key_span, name.as_str())
                    }

                    // Dynamic values are never resolvable (as their names are
                    // of course only known at runtime).
                    //
                    // Note: This branch is unreachable in `let`-expressions.
                    KeySlot::Dynamic { .. } => self.scope_mut().declare_phantom(key_span, false),
                },

                // In non-recursive attribute sets, the value is inaccessible
                // (only consumed by `OpAttrs`).
                BindingsKind::Attrs => self.scope_mut().declare_phantom(key_span, false),
            };

            let binding = if path.peek().is_some() {
                Binding::Set(AttributeSet {
                    span,
                    kind: BindingsKind::Attrs,
                    inherits: vec![],
                    entries: vec![(span, path, value)],
                })
            } else {
                Binding::Plain { expr: value }
            };

            bindings.track_new(key_slot, value_slot, binding);
        }
    }

    /// Compile attribute set literals into equivalent bytecode.
    ///
    /// This is complicated by a number of features specific to Nix attribute
    /// sets, most importantly:
    ///
    /// 1. Keys can be dynamically constructed through interpolation.
    /// 2. Keys can refer to nested attribute sets.
    /// 3. Attribute sets can (optionally) be recursive.
    pub(super) fn compile_attr_set(&mut self, slot: LocalIdx, node: &ast::AttrSet) {
        // Open a scope to track the positions of the temporaries used by the
        // `OpAttrs` instruction.
        self.scope_mut().begin_scope();

        let kind = if node.rec_token().is_some() {
            BindingsKind::RecAttrs
        } else {
            BindingsKind::Attrs
        };

        self.compile_bindings(slot, kind, node);

        // Remove the temporary scope, but do not emit any additional cleanup
        // (OpAttrs consumes all of these locals).
        self.scope_mut().end_scope();
    }

    /// Actually binds all tracked bindings by emitting the bytecode that places
    /// them in their stack slots.
    fn bind_values(&mut self, bindings: TrackedBindings) {
        let mut value_indices: Vec<LocalIdx> = vec![];

        for binding in bindings.bindings.into_iter() {
            value_indices.push(binding.value_slot);

            match binding.key_slot {
                KeySlot::None { .. } => {} // nothing to do here

                KeySlot::Static { slot, name } => {
                    let span = self.scope()[slot].span;
                    self.emit_constant(Value::String(name.into()), &span);
                    self.scope_mut().mark_initialised(slot);
                }

                KeySlot::Dynamic { slot, attr } => {
                    self.compile_attr(slot, &attr);
                    self.scope_mut().mark_initialised(slot);
                }
            }

            match binding.binding {
                // This entry is an inherit (from) expr. The value is placed on
                // the stack by selecting an attribute.
                Binding::InheritFrom {
                    namespace,
                    name,
                    span,
                } => {
                    // Create a thunk wrapping value (which may be one as well)
                    // to avoid forcing the from expr too early.
                    self.thunk(binding.value_slot, &namespace, |c, s| {
                        c.compile(s, &namespace);
                        c.emit_force(&namespace);

                        c.emit_constant(Value::String(name.into()), &span);
                        c.push_op(OpCode::OpAttrsSelect, &span);
                    })
                }

                // Binding is "just" a plain expression that needs to be
                // compiled.
                Binding::Plain { expr } => self.compile(binding.value_slot, &expr),

                // Binding is a merged or nested attribute set, and needs to be
                // recursively compiled as another binding.
                Binding::Set(set) => self.thunk(binding.value_slot, &set, |c, _| {
                    c.scope_mut().begin_scope();
                    c.compile_bindings(binding.value_slot, set.kind, &set);
                    c.scope_mut().end_scope();
                }),
            }

            // Any code after this point will observe the value in the right
            // stack slot, so mark it as initialised.
            self.scope_mut().mark_initialised(binding.value_slot);
        }

        // Final pass to emit finaliser instructions if necessary.
        for idx in value_indices {
            if self.scope()[idx].needs_finaliser {
                let stack_idx = self.scope().stack_index(idx);
                let span = self.scope()[idx].span;
                self.push_op(OpCode::OpFinalise(stack_idx), &span);
            }
        }
    }

    fn compile_bindings<N>(&mut self, slot: LocalIdx, kind: BindingsKind, node: &N)
    where
        N: ToSpan + HasEntryProxy,
    {
        let mut count = 0;
        self.scope_mut().begin_scope();

        // Vector to track all observed bindings.
        let mut bindings = TrackedBindings::new();

        let inherit_froms = self.compile_plain_inherits(slot, kind, &mut count, node);
        self.declare_namespaced_inherits(kind, inherit_froms, &mut bindings);
        self.declare_bindings(kind, &mut count, &mut bindings, node);

        // Actually bind values and ensure they are on the stack.
        self.bind_values(bindings);

        if kind.is_attrs() {
            self.push_op(OpCode::OpAttrs(Count(count)), node);
        }
    }

    /// Compile a standard `let ...; in ...` expression.
    ///
    /// Unless in a non-standard scope, the encountered values are simply pushed
    /// on the stack and their indices noted in the entries vector.
    pub(super) fn compile_let_in(&mut self, slot: LocalIdx, node: &ast::LetIn) {
        self.compile_bindings(slot, BindingsKind::LetIn, node);

        // Deal with the body, then clean up the locals afterwards.
        self.compile(slot, &node.body().unwrap());
        self.cleanup_scope(node);
    }

    pub(super) fn compile_legacy_let(&mut self, slot: LocalIdx, node: &ast::LegacyLet) {
        self.emit_warning(node, WarningKind::DeprecatedLegacyLet);
        self.scope_mut().begin_scope();
        self.compile_bindings(slot, BindingsKind::RecAttrs, node);

        // Remove the temporary scope, but do not emit any additional cleanup
        // (OpAttrs consumes all of these locals).
        self.scope_mut().end_scope();

        self.emit_constant(Value::String(SmolStr::new_inline("body").into()), node);
        self.push_op(OpCode::OpAttrsSelect, node);
    }

    /// Resolve and compile access to an identifier in the scope.
    fn compile_identifier_access<N: ToSpan + Clone>(
        &mut self,
        slot: LocalIdx,
        ident: &str,
        node: &N,
    ) {
        // If the identifier is a global, and it is not poisoned, emit the
        // global directly.
        if let Some(global) = self.globals.get(ident) {
            if !self.scope().is_poisoned(ident) {
                global.clone()(self, self.span_for(node));
                return;
            }
        }

        match self.scope_mut().resolve_local(ident) {
            LocalPosition::Unknown => {
                // Are we possibly dealing with an upvalue?
                if let Some(idx) = self.resolve_upvalue(self.contexts.len() - 1, ident, node) {
                    self.push_op(OpCode::OpGetUpvalue(idx), node);
                    return;
                }

                // If there is a non-empty `with`-stack (or a parent context
                // with one), emit a runtime dynamic resolution instruction.
                //
                // Since it is possible for users to e.g. assign a variable to a
                // dynamic resolution without actually using it, this operation
                // is wrapped in an extra thunk.
                if self.has_dynamic_ancestor() {
                    self.thunk(slot, node, |c, _| {
                        c.context_mut().captures_with_stack = true;
                        c.emit_constant(Value::String(SmolStr::new(ident).into()), node);
                        c.push_op(OpCode::OpResolveWith, node);
                    });
                    return;
                }

                // Otherwise, this variable is missing.
                self.emit_error(node, ErrorKind::UnknownStaticVariable);
            }

            LocalPosition::Known(idx) => {
                let stack_idx = self.scope().stack_index(idx);
                self.push_op(OpCode::OpGetLocal(stack_idx), node);
            }

            // This identifier is referring to a value from the same scope which
            // is not yet defined. This identifier access must be thunked.
            LocalPosition::Recursive(idx) => self.thunk(slot, node, move |compiler, _| {
                let upvalue_idx = compiler.add_upvalue(
                    compiler.contexts.len() - 1,
                    node,
                    UpvalueKind::Local(idx),
                );
                compiler.push_op(OpCode::OpGetUpvalue(upvalue_idx), node);
            }),
        };
    }

    pub(super) fn compile_ident(&mut self, slot: LocalIdx, node: &ast::Ident) {
        let ident = node.ident_token().unwrap();
        self.compile_identifier_access(slot, ident.text(), node);
    }
}

/// Private compiler helpers related to bindings.
impl Compiler<'_> {
    fn resolve_upvalue<N: ToSpan>(
        &mut self,
        ctx_idx: usize,
        name: &str,
        node: &N,
    ) -> Option<UpvalueIdx> {
        if ctx_idx == 0 {
            // There can not be any upvalue at the outermost context.
            return None;
        }

        // Determine whether the upvalue is a local in the enclosing context.
        match self.contexts[ctx_idx - 1].scope.resolve_local(name) {
            // recursive upvalues are dealt with the same way as standard known
            // ones, as thunks and closures are guaranteed to be placed on the
            // stack (i.e. in the right position) *during* their runtime
            // construction
            LocalPosition::Known(idx) | LocalPosition::Recursive(idx) => {
                return Some(self.add_upvalue(ctx_idx, node, UpvalueKind::Local(idx)))
            }

            LocalPosition::Unknown => { /* continue below */ }
        };

        // If the upvalue comes from even further up, we need to recurse to make
        // sure that the upvalues are created at each level.
        if let Some(idx) = self.resolve_upvalue(ctx_idx - 1, name, node) {
            return Some(self.add_upvalue(ctx_idx, node, UpvalueKind::Upvalue(idx)));
        }

        None
    }

    fn add_upvalue<N: ToSpan>(
        &mut self,
        ctx_idx: usize,
        node: &N,
        kind: UpvalueKind,
    ) -> UpvalueIdx {
        // If there is already an upvalue closing over the specified index,
        // retrieve that instead.
        for (idx, existing) in self.contexts[ctx_idx].scope.upvalues.iter().enumerate() {
            if existing.kind == kind {
                return UpvalueIdx(idx);
            }
        }

        let span = self.span_for(node);
        self.contexts[ctx_idx]
            .scope
            .upvalues
            .push(Upvalue { kind, span });

        let idx = UpvalueIdx(self.contexts[ctx_idx].lambda.upvalue_count);
        self.contexts[ctx_idx].lambda.upvalue_count += 1;
        idx
    }

    /// Convert a non-dynamic string expression to a string if possible.
    fn expr_static_str(&self, node: &ast::Str) -> Option<SmolStr> {
        let mut parts = node.normalized_parts();

        if parts.len() != 1 {
            return None;
        }

        if let Some(ast::InterpolPart::Literal(lit)) = parts.pop() {
            return Some(SmolStr::new(lit));
        }

        None
    }

    /// Convert the provided `ast::Attr` into a statically known string if
    /// possible.
    fn expr_static_attr_str(&self, node: &ast::Attr) -> Option<SmolStr> {
        match node {
            ast::Attr::Ident(ident) => Some(ident.ident_token().unwrap().text().into()),
            ast::Attr::Str(s) => self.expr_static_str(s),

            // The dynamic node type is just a wrapper. C++ Nix does not care
            // about the dynamic wrapper when determining whether the node
            // itself is dynamic, it depends solely on the expression inside
            // (i.e. `let ${"a"} = 1; in a` is valid).
            ast::Attr::Dynamic(ref dynamic) => match dynamic.expr().unwrap() {
                ast::Expr::Str(s) => self.expr_static_str(&s),
                _ => None,
            },
        }
    }
}
