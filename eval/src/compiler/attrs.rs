//! This module implements compiler logic related to attribute sets
//! (construction, access operators, ...).

use super::*;

impl Compiler<'_, '_> {
    pub(super) fn compile_attr(&mut self, slot: LocalIdx, node: ast::Attr) {
        match node {
            ast::Attr::Dynamic(dynamic) => {
                self.compile(slot, dynamic.expr().unwrap());
                self.emit_force(&dynamic.expr().unwrap());
            }

            ast::Attr::Str(s) => {
                self.compile_str(slot, s.clone());
                self.emit_force(&s);
            }

            ast::Attr::Ident(ident) => self.emit_literal_ident(&ident),
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
        if node.rec_token().is_some() {
            let span = self.span_for(&node);
            self.emit_warning(
                span,
                WarningKind::NotImplemented("recursive attribute sets"),
            );
        }

        // Open a scope to track the positions of the temporaries used
        // by the `OpAttrs` instruction.
        self.scope_mut().begin_scope();

        let mut count = self.compile_inherit_attrs(slot, node.inherits());

        for kv in node.attrpath_values() {
            count += 1;

            // Because attribute set literals can contain nested keys,
            // there is potentially more than one key fragment. If
            // this is the case, a special operation to construct a
            // runtime value representing the attribute path is
            // emitted.
            let mut key_count = 0;
            let key_span = self.span_for(&kv.attrpath().unwrap());
            let key_idx = self.scope_mut().declare_phantom(key_span, false);

            for fragment in kv.attrpath().unwrap().attrs() {
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
                    &kv.attrpath().unwrap(),
                );

                // Close the temporary scope that was set up for the
                // key fragments.
                self.scope_mut().end_scope();
            }

            // The value is just compiled as normal so that its
            // resulting value is on the stack when the attribute set
            // is constructed at runtime.
            let value_span = self.span_for(&kv.value().unwrap());
            let value_slot = self.scope_mut().declare_phantom(value_span, false);
            self.compile(value_slot, kv.value().unwrap());
            self.scope_mut().mark_initialised(value_slot);
        }

        self.push_op(OpCode::OpAttrs(Count(count)), &node);

        // Remove the temporary scope, but do not emit any additional
        // cleanup (OpAttrs consumes all of these locals).
        self.scope_mut().end_scope();
    }

    pub(super) fn compile_has_attr(&mut self, slot: LocalIdx, node: ast::HasAttr) {
        // Put the attribute set on the stack.
        self.compile(slot, node.expr().unwrap());

        // Push all path fragments with an operation for fetching the
        // next nested element, for all fragments except the last one.
        for (count, fragment) in node.attrpath().unwrap().attrs().enumerate() {
            if count > 0 {
                self.push_op(OpCode::OpAttrsTrySelect, &fragment);
            }

            self.compile_attr(slot, fragment);
        }

        // After the last fragment, emit the actual instruction that
        // leaves a boolean on the stack.
        self.push_op(OpCode::OpAttrsIsSet, &node);
    }

    pub(super) fn compile_select(&mut self, slot: LocalIdx, node: ast::Select) {
        let set = node.expr().unwrap();
        let path = node.attrpath().unwrap();

        if node.or_token().is_some() {
            self.compile_select_or(slot, set, path, node.default_expr().unwrap());
            return;
        }

        // Push the set onto the stack
        self.compile(slot, set.clone());
        self.emit_force(&set);

        // Compile each key fragment and emit access instructions.
        //
        // TODO: multi-select instruction to avoid re-pushing attrs on
        // nested selects.
        for fragment in path.attrs() {
            self.compile_attr(slot, fragment.clone());
            self.push_op(OpCode::OpAttrsSelect, &fragment);
        }
    }

    /// Compile an `or` expression into a chunk of conditional jumps.
    ///
    /// If at any point during attribute set traversal a key is
    /// missing, the `OpAttrOrNotFound` instruction will leave a
    /// special sentinel value on the stack.
    ///
    /// After each access, a conditional jump evaluates the top of the
    /// stack and short-circuits to the default value if it sees the
    /// sentinel.
    ///
    /// Code like `{ a.b = 1; }.a.c or 42` yields this bytecode and
    /// runtime stack:
    ///
    /// ```notrust
    ///            Bytecode                     Runtime stack
    ///  ┌────────────────────────────┐   ┌─────────────────────────┐
    ///  │    ...                     │   │ ...                     │
    ///  │ 5  OP_ATTRS(1)             │ → │ 5  [ { a.b = 1; }     ] │
    ///  │ 6  OP_CONSTANT("a")        │ → │ 6  [ { a.b = 1; } "a" ] │
    ///  │ 7  OP_ATTR_OR_NOT_FOUND    │ → │ 7  [ { b = 1; }       ] │
    ///  │ 8  JUMP_IF_NOT_FOUND(13)   │ → │ 8  [ { b = 1; }       ] │
    ///  │ 9  OP_CONSTANT("C")        │ → │ 9  [ { b = 1; } "c"   ] │
    ///  │ 10 OP_ATTR_OR_NOT_FOUND    │ → │ 10 [ NOT_FOUND        ] │
    ///  │ 11 JUMP_IF_NOT_FOUND(13)   │ → │ 11 [                  ] │
    ///  │ 12 JUMP(14)                │   │ ..     jumped over      │
    ///  │ 13 CONSTANT(42)            │ → │ 12 [ 42 ]               │
    ///  │ 14 ...                     │   │ ..   ....               │
    ///  └────────────────────────────┘   └─────────────────────────┘
    /// ```
    fn compile_select_or(
        &mut self,
        slot: LocalIdx,
        set: ast::Expr,
        path: ast::Attrpath,
        default: ast::Expr,
    ) {
        self.compile(slot, set.clone());
        self.emit_force(&set);
        let mut jumps = vec![];

        for fragment in path.attrs() {
            self.compile_attr(slot, fragment.clone());
            self.push_op(OpCode::OpAttrsTrySelect, &fragment);
            jumps.push(self.push_op(OpCode::OpJumpIfNotFound(JumpOffset(0)), &fragment));
        }

        let final_jump = self.push_op(OpCode::OpJump(JumpOffset(0)), &path);

        for jump in jumps {
            self.patch_jump(jump);
        }

        // Compile the default value expression and patch the final
        // jump to point *beyond* it.
        self.compile(slot, default);
        self.patch_jump(final_jump);
    }
}
