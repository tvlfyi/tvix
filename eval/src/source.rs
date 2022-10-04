//! This module contains utilities for dealing with the codemap that
//! needs to be carried across different compiler instantiations in an
//! evaluation.
//!
//! The data type `SourceCode` should be carried through all relevant
//! places instead of copying the codemap structures directly.

use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
    sync::Arc,
};

use codemap::{CodeMap, Span};

/// Tracks all source code in a Tvix evaluation for accurate error
/// reporting.
#[derive(Clone)]
pub struct SourceCode(Rc<RefCell<CodeMap>>);

impl SourceCode {
    /// Create a new SourceCode instance.
    pub fn new() -> Self {
        SourceCode(Rc::new(RefCell::new(CodeMap::new())))
    }

    /// Access a read-only reference to the codemap.
    pub fn codemap(&self) -> Ref<CodeMap> {
        self.0.borrow()
    }

    /// Access a writable reference to the codemap.
    fn codemap_mut(&self) -> RefMut<CodeMap> {
        self.0.borrow_mut()
    }

    /// Add a file to the codemap. The returned Arc is managed by the
    /// codemap internally and can be used like a normal reference.
    pub fn add_file(&self, name: String, code: String) -> Arc<codemap::File> {
        self.codemap_mut().add_file(name, code)
    }

    /// Retrieve the line number of the given span. If it spans
    /// multiple lines, the first line will be returned.
    pub fn get_line(&self, span: Span) -> usize {
        // lines are 0-indexed in the codemap, but users probably want
        // real line numbers
        self.codemap().look_up_span(span).begin.line + 1
    }

    /// Returns the literal source slice of the given span.
    pub fn source_slice(&self, span: Span) -> Ref<str> {
        Ref::map(self.codemap(), |c| {
            c.find_file(span.low()).source_slice(span)
        })
    }
}
