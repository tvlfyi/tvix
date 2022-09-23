//! This module implements compiler logic related to name/value
//! binding definitions (that is, attribute sets and let-expressions).
//!
//! In the case of recursive scopes these cases share almost all of
//! their (fairly complex) logic.

use super::*;

// Data structures to track the bindings observed in the
// second pass, and forward the information needed to compile
// their value.
enum BindingKind {
    InheritFrom {
        namespace: ast::Expr,
        name: SmolStr,
        span: Span,
    },

    Plain {
        expr: ast::Expr,
    },
}

struct KeySlot {
    slot: LocalIdx,
    name: SmolStr,
}

struct TrackedBinding {
    key_slot: Option<KeySlot>,
    value_slot: LocalIdx,
    kind: BindingKind,
}

/// AST-traversing functions related to bindings.
impl Compiler<'_> {
    /// Compiles inherited values in an attribute set. Inherited
    /// values are *always* inherited from the outer scope, even if
    /// there is a matching name within a recursive attribute set.
    fn compile_inherit_attrs(
        &mut self,
        slot: LocalIdx,
        inherits: AstChildren<ast::Inherit>,
    ) -> usize {
        // Count the number of inherited values, so that the outer
        // constructor can emit the correct number of pairs when
        // constructing attribute sets.
        let mut count = 0;

        for inherit in inherits {
            match inherit.from() {
                Some(from) => {
                    for attr in inherit.attrs() {
                        count += 1;

                        let name = match self.expr_static_attr_str(&attr) {
                            Some(name) => name,
                            None => {
                                // TODO(tazjin): error variant for dynamic
                                // key in *inherit* (or generalise it)
                                self.emit_error(&attr, ErrorKind::DynamicKeyInLet);
                                continue;
                            }
                        };

                        let name_span = self.span_for(&attr);

                        // First emit the identifier itself (this
                        // becomes the new key).
                        self.emit_constant(Value::String(SmolStr::new(&name).into()), &attr);
                        self.scope_mut().declare_phantom(name_span, true);

                        // Then emit the node that we're inheriting
                        // from.
                        //
                        // TODO: Likely significant optimisation
                        // potential in having a multi-select
                        // instruction followed by a merge, rather
                        // than pushing/popping the same attrs
                        // potentially a lot of times.
                        let val_idx = self.scope_mut().declare_phantom(name_span, false);
                        self.compile(val_idx, from.expr().unwrap());
                        self.emit_force(&from.expr().unwrap());
                        self.emit_constant(Value::String(name.into()), &attr);
                        self.push_op(OpCode::OpAttrsSelect, &attr);
                        self.scope_mut().mark_initialised(val_idx);
                    }
                }

                None => {
                    for attr in inherit.attrs() {
                        count += 1;

                        // Emit the key to use for OpAttrs
                        let name = match self.expr_static_attr_str(&attr) {
                            Some(name) => name,
                            None => {
                                // TODO(tazjin): error variant for dynamic
                                // key in *inherit* (or generalise it)
                                self.emit_error(&attr, ErrorKind::DynamicKeyInLet);
                                continue;
                            }
                        };

                        let name_span = self.span_for(&attr);
                        self.emit_constant(Value::String(SmolStr::new(&name).into()), &attr);
                        self.scope_mut().declare_phantom(name_span, true);

                        // Emit the value.
                        self.compile_identifier_access(slot, &name, &attr);
                        self.scope_mut().declare_phantom(name_span, true);
                    }
                }
            }
        }

        count
    }

    /// Compile the statically known entries of an attribute set. Which
    /// keys are which is not known from the iterator, so discovered
    /// dynamic keys are returned from here.
    fn compile_static_attr_entries(
        &mut self,
        count: &mut usize,
        entries: AstChildren<ast::AttrpathValue>,
    ) -> Vec<ast::AttrpathValue> {
        let mut dynamic_attrs: Vec<ast::AttrpathValue> = vec![];

        'entries: for kv in entries {
            // Attempt to turn the attrpath into a list of static
            // strings, but abort this process if any dynamic
            // fragments are encountered.
            let static_attrpath: Option<Vec<String>> = kv
                .attrpath()
                .unwrap()
                .attrs()
                .map(|a| self.expr_static_attr_str(&a))
                .collect();

            let fragments = match static_attrpath {
                Some(fragments) => fragments,
                None => {
                    dynamic_attrs.push(kv);
                    continue 'entries;
                }
            };

            // At this point we can increase the counter because we
            // know that this particular attribute is static and can
            // thus be processed here.
            *count += 1;

            let key_count = fragments.len();
            for fragment in fragments.into_iter() {
                self.emit_constant(Value::String(fragment.into()), &kv.attrpath().unwrap());
            }

            // We're done with the key if there was only one fragment,
            // otherwise we need to emit an instruction to construct
            // the attribute path.
            if key_count > 1 {
                self.push_op(
                    OpCode::OpAttrPath(Count(key_count)),
                    &kv.attrpath().unwrap(),
                );
            }

            // The value is just compiled as normal so that its
            // resulting value is on the stack when the attribute set
            // is constructed at runtime.
            let value_span = self.span_for(&kv.value().unwrap());
            let value_slot = self.scope_mut().declare_phantom(value_span, false);
            self.compile(value_slot, kv.value().unwrap());
            self.scope_mut().mark_initialised(value_slot);
        }

        dynamic_attrs
    }

    /// Compile the dynamic entries of an attribute set, where keys
    /// are only known at runtime.
    fn compile_dynamic_attr_entries(
        &mut self,
        count: &mut usize,
        entries: Vec<ast::AttrpathValue>,
    ) {
        for entry in entries.into_iter() {
            *count += 1;

            let mut key_count = 0;
            let key_span = self.span_for(&entry.attrpath().unwrap());
            let key_idx = self.scope_mut().declare_phantom(key_span, false);

            for fragment in entry.attrpath().unwrap().attrs() {
                // Key fragments can contain dynamic expressions,
                // which makes accounting for their stack slots very
                // tricky.
                //
                // In order to ensure the locals are correctly cleaned
                // up, we essentially treat any key fragment after the
                // first one (which has a locals index that will
                // become that of the final key itself) as being part
                // of a separate scope, which results in the somewhat
                // annoying setup logic below.
                let fragment_slot = match key_count {
                    0 => key_idx,
                    1 => {
                        self.scope_mut().begin_scope();
                        self.scope_mut().declare_phantom(key_span, false)
                    }
                    _ => self.scope_mut().declare_phantom(key_span, false),
                };

                key_count += 1;
                self.compile_attr(fragment_slot, fragment);
                self.scope_mut().mark_initialised(fragment_slot);
            }

            // We're done with the key if there was only one fragment,
            // otherwise we need to emit an instruction to construct
            // the attribute path.
            if key_count > 1 {
                self.push_op(
                    OpCode::OpAttrPath(Count(key_count)),
                    &entry.attrpath().unwrap(),
                );

                // Close the temporary scope that was set up for the
                // key fragments.
                self.scope_mut().end_scope();
            }

            // The value is just compiled as normal so that its
            // resulting value is on the stack when the attribute set
            // is constructed at runtime.
            let value_span = self.span_for(&entry.value().unwrap());
            let value_slot = self.scope_mut().declare_phantom(value_span, false);
            self.compile(value_slot, entry.value().unwrap());
            self.scope_mut().mark_initialised(value_slot);
        }
    }

    /// Compile attribute set literals into equivalent bytecode.
    ///
    /// This is complicated by a number of features specific to Nix
    /// attribute sets, most importantly:
    ///
    /// 1. Keys can be dynamically constructed through interpolation.
    /// 2. Keys can refer to nested attribute sets.
    /// 3. Attribute sets can (optionally) be recursive.
    pub(super) fn compile_attr_set(&mut self, slot: LocalIdx, node: ast::AttrSet) {
        // Open a scope to track the positions of the temporaries used
        // by the `OpAttrs` instruction.
        self.scope_mut().begin_scope();

        if node.rec_token().is_some() {
            let count = self.compile_recursive_scope(slot, true, &node);
            self.push_op(OpCode::OpAttrs(Count(count)), &node);
        } else {
            let mut count = self.compile_inherit_attrs(slot, node.inherits());

            let dynamic_entries =
                self.compile_static_attr_entries(&mut count, node.attrpath_values());

            self.compile_dynamic_attr_entries(&mut count, dynamic_entries);

            self.push_op(OpCode::OpAttrs(Count(count)), &node);
        }

        // Remove the temporary scope, but do not emit any additional
        // cleanup (OpAttrs consumes all of these locals).
        self.scope_mut().end_scope();
    }

    fn compile_recursive_scope<N>(&mut self, slot: LocalIdx, rec_attrs: bool, node: &N) -> usize
    where
        N: ToSpan + ast::HasEntry,
    {
        let mut count = 0;
        self.scope_mut().begin_scope();

        // First pass to find all plain inherits (if they are not useless).
        // Since they always resolve to a higher scope, we can just compile and
        // declare them immediately. This needs to happen *before* we declare
        // any other locals in the scope or the stack order gets messed up.
        // While we are looping through the inherits, already note all inherit
        // (from) expressions, that may very well resolve recursively and need
        // to be compiled like normal let in bindings.
        let mut inherit_froms: Vec<(ast::Expr, String, Span)> = vec![];
        for inherit in node.inherits() {
            match inherit.from() {
                // Within a `let` binding, inheriting from the outer
                // scope is a no-op *if* the identifier can be
                // statically resolved.
                None if !rec_attrs && !self.scope().has_with() => {
                    self.emit_warning(&inherit, WarningKind::UselessInherit);
                    continue;
                }

                None => {
                    for attr in inherit.attrs() {
                        let name = match self.expr_static_attr_str(&attr) {
                            Some(name) => name,
                            None => {
                                // TODO(tazjin): error variant for dynamic
                                // key in *inherit* (or generalise it)
                                self.emit_error(&attr, ErrorKind::DynamicKeyInLet);
                                continue;
                            }
                        };

                        count += 1;

                        // If the identifier resolves statically in a
                        // `let`, it has precedence over dynamic
                        // bindings, and the inherit is useless.
                        if !rec_attrs
                            && matches!(
                                self.scope_mut().resolve_local(&name),
                                LocalPosition::Known(_)
                            )
                        {
                            self.emit_warning(&attr, WarningKind::UselessInherit);
                            continue;
                        }

                        if rec_attrs {
                            self.emit_constant(Value::String(SmolStr::new(&name).into()), &attr);
                            let span = self.span_for(&attr);
                            self.scope_mut().declare_phantom(span, true);
                        }

                        self.compile_identifier_access(slot, &name, &attr);
                        let idx = self.declare_local(&attr, &name);
                        self.scope_mut().mark_initialised(idx);
                    }
                }

                Some(from) => {
                    for attr in inherit.attrs() {
                        let name = match self.expr_static_attr_str(&attr) {
                            Some(name) => name,
                            None => {
                                // TODO(tazjin): error variant for dynamic
                                // key in *inherit* (or generalise it)
                                self.emit_error(&attr, ErrorKind::DynamicKeyInLet);
                                continue;
                            }
                        };

                        count += 1;
                        inherit_froms.push((from.expr().unwrap(), name, self.span_for(&attr)));
                    }
                }
            }
        }

        // Vector to track these observed bindings.
        let mut bindings: Vec<TrackedBinding> = vec![];

        // Begin second pass to ensure that all remaining identifiers
        // (that may resolve recursively) are known.

        // Begin with the inherit (from)s since they all become a thunk anyway
        for (from, name, span) in inherit_froms {
            let key_slot = if rec_attrs {
                Some(KeySlot {
                    slot: self.scope_mut().declare_phantom(span, false),
                    name: SmolStr::new(&name),
                })
            } else {
                None
            };

            let value_slot = self.declare_local(&span, &name);

            bindings.push(TrackedBinding {
                key_slot,
                value_slot,
                kind: BindingKind::InheritFrom {
                    namespace: from,
                    name: SmolStr::new(&name),
                    span,
                },
            });
        }

        // Declare all regular bindings
        for entry in node.attrpath_values() {
            count += 1;

            let mut path = match self.normalise_ident_path(entry.attrpath().unwrap().attrs()) {
                Ok(p) => p,
                Err(err) => {
                    self.errors.push(err);
                    continue;
                }
            };

            if path.len() != 1 {
                self.emit_error(
                    &entry,
                    ErrorKind::NotImplemented("nested bindings in recursive scope :("),
                );
                continue;
            }

            let key_slot = if rec_attrs {
                let span = self.span_for(&entry.attrpath().unwrap());
                Some(KeySlot {
                    slot: self.scope_mut().declare_phantom(span, false),
                    name: SmolStr::new(&path[0]),
                })
            } else {
                None
            };

            let value_slot = self.declare_local(&entry.attrpath().unwrap(), path.pop().unwrap());

            bindings.push(TrackedBinding {
                key_slot,
                value_slot,
                kind: BindingKind::Plain {
                    expr: entry.value().unwrap(),
                },
            });
        }

        // Third pass to place the values in the correct stack slots.
        let mut value_indices: Vec<LocalIdx> = vec![];
        for binding in bindings.into_iter() {
            value_indices.push(binding.value_slot);

            if let Some(key_slot) = binding.key_slot {
                let span = self.scope()[key_slot.slot].span;
                self.emit_constant(Value::String(key_slot.name.into()), &span);
                self.scope_mut().mark_initialised(key_slot.slot);
            }

            match binding.kind {
                // This entry is an inherit (from) expr. The value is
                // placed on the stack by selecting an attribute.
                BindingKind::InheritFrom {
                    namespace,
                    name,
                    span,
                } => {
                    // Create a thunk wrapping value (which may be one as well) to
                    // avoid forcing the from expr too early.
                    self.thunk(binding.value_slot, &namespace, move |c, n, s| {
                        c.compile(s, n.clone());
                        c.emit_force(n);

                        c.emit_constant(Value::String(name.into()), &span);
                        c.push_op(OpCode::OpAttrsSelect, &span);
                    })
                }

                // Binding is "just" a plain expression that needs to
                // be compiled.
                BindingKind::Plain { expr } => self.compile(binding.value_slot, expr),
            }

            // Any code after this point will observe the value in the
            // right stack slot, so mark it as initialised.
            self.scope_mut().mark_initialised(binding.value_slot);
        }

        // Fourth pass to emit finaliser instructions if necessary.
        for idx in value_indices {
            if self.scope()[idx].needs_finaliser {
                let stack_idx = self.scope().stack_index(idx);
                self.push_op(OpCode::OpFinalise(stack_idx), node);
            }
        }

        count
    }

    /// Compile a standard `let ...; in ...` expression.
    ///
    /// Unless in a non-standard scope, the encountered values are
    /// simply pushed on the stack and their indices noted in the
    /// entries vector.
    pub(super) fn compile_let_in(&mut self, slot: LocalIdx, node: ast::LetIn) {
        self.compile_recursive_scope(slot, false, &node);

        // Deal with the body, then clean up the locals afterwards.
        self.compile(slot, node.body().unwrap());
        self.cleanup_scope(&node);
    }

    pub(super) fn compile_legacy_let(&mut self, slot: LocalIdx, node: ast::LegacyLet) {
        self.emit_warning(&node, WarningKind::DeprecatedLegacyLet);
        self.scope_mut().begin_scope();
        self.compile_recursive_scope(slot, true, &node);
        self.push_op(OpCode::OpAttrs(Count(node.entries().count())), &node);
        self.emit_constant(Value::String(SmolStr::new_inline("body").into()), &node);
        self.push_op(OpCode::OpAttrsSelect, &node);
    }

    /// Resolve and compile access to an identifier in the scope.
    fn compile_identifier_access<N: ToSpan + Clone>(
        &mut self,
        slot: LocalIdx,
        ident: &str,
        node: &N,
    ) {
        // If the identifier is a global, and it is not poisoned, emit
        // the global directly.
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

                // If there is a non-empty `with`-stack (or a parent
                // context with one), emit a runtime dynamic
                // resolution instruction.
                if self.has_dynamic_ancestor() {
                    self.emit_constant(Value::String(SmolStr::new(ident).into()), node);
                    self.push_op(OpCode::OpResolveWith, node);
                    return;
                }

                // Otherwise, this variable is missing.
                self.emit_error(node, ErrorKind::UnknownStaticVariable);
            }

            LocalPosition::Known(idx) => {
                let stack_idx = self.scope().stack_index(idx);
                self.push_op(OpCode::OpGetLocal(stack_idx), node);
            }

            // This identifier is referring to a value from the same
            // scope which is not yet defined. This identifier access
            // must be thunked.
            LocalPosition::Recursive(idx) => self.thunk(slot, node, move |compiler, node, _| {
                let upvalue_idx = compiler.add_upvalue(
                    compiler.contexts.len() - 1,
                    node,
                    UpvalueKind::Local(idx),
                );
                compiler.push_op(OpCode::OpGetUpvalue(upvalue_idx), node);
            }),
        };
    }

    pub(super) fn compile_ident(&mut self, slot: LocalIdx, node: ast::Ident) {
        let ident = node.ident_token().unwrap();
        self.compile_identifier_access(slot, ident.text(), &node);
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
            // recursive upvalues are dealt with the same way as
            // standard known ones, as thunks and closures are
            // guaranteed to be placed on the stack (i.e. in the right
            // position) *during* their runtime construction
            LocalPosition::Known(idx) | LocalPosition::Recursive(idx) => {
                return Some(self.add_upvalue(ctx_idx, node, UpvalueKind::Local(idx)))
            }

            LocalPosition::Unknown => { /* continue below */ }
        };

        // If the upvalue comes from even further up, we need to
        // recurse to make sure that the upvalues are created at each
        // level.
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
        // If there is already an upvalue closing over the specified
        // index, retrieve that instead.
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

    /// Convert a single identifier path fragment of a let binding to
    /// a string if possible, or raise an error about the node being
    /// dynamic.
    fn binding_name(&self, node: ast::Attr) -> EvalResult<String> {
        self.expr_static_attr_str(&node)
            .ok_or_else(|| self.dynamic_key_error(&node))
    }

    /// Normalises identifier fragments into a single string vector
    /// for `let`-expressions; fails if fragments requiring dynamic
    /// computation are encountered.
    fn normalise_ident_path<I: Iterator<Item = ast::Attr>>(
        &self,
        path: I,
    ) -> EvalResult<Vec<String>> {
        path.map(|node| self.binding_name(node)).collect()
    }

    /// Construct the error returned when a dynamic attribute is found
    /// in a `let`-expression.
    fn dynamic_key_error<N>(&self, node: &N) -> Error
    where
        N: ToSpan,
    {
        Error {
            kind: ErrorKind::DynamicKeyInLet,
            span: self.span_for(node),
        }
    }

    /// Convert a non-dynamic string expression to a string if possible.
    fn expr_static_str(&self, node: &ast::Str) -> Option<String> {
        let mut parts = node.normalized_parts();

        if parts.len() != 1 {
            return None;
        }

        if let Some(ast::InterpolPart::Literal(lit)) = parts.pop() {
            return Some(lit);
        }

        None
    }

    /// Convert the provided `ast::Attr` into a statically known
    /// string if possible.
    // TODO(tazjin): these should probably be SmolStr
    fn expr_static_attr_str(&self, node: &ast::Attr) -> Option<String> {
        match node {
            ast::Attr::Ident(ident) => Some(ident.ident_token().unwrap().text().into()),
            ast::Attr::Str(s) => self.expr_static_str(s),

            // The dynamic node type is just a wrapper. C++ Nix does not
            // care about the dynamic wrapper when determining whether the
            // node itself is dynamic, it depends solely on the expression
            // inside (i.e. `let ${"a"} = 1; in a` is valid).
            ast::Attr::Dynamic(ref dynamic) => match dynamic.expr().unwrap() {
                ast::Expr::Str(s) => self.expr_static_str(&s),
                _ => None,
            },
        }
    }
}
