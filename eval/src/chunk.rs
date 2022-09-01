use crate::opcode::{CodeIdx, ConstantIdx, OpCode};
use crate::value::Value;

/// Represents a source location from which one or more operations
/// were compiled.
///
/// The span itself is an index into a [codemap::Codemap], and the
/// structure tracks the number of operations that were yielded from
/// the same span.
///
/// At error reporting time, it becomes possible to either just fetch
/// the textual representation of that span from the codemap, or to
/// even re-parse the AST using rnix to create more semantically
/// interesting errors.
#[derive(Clone, Debug)]
struct SourceSpan {
    /// Span into the [codemap::Codemap].
    span: codemap::Span,

    /// Number of instructions derived from this span.
    count: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<Value>,
    spans: Vec<SourceSpan>,
}

impl Chunk {
    pub fn push_op(&mut self, data: OpCode) -> CodeIdx {
        let idx = self.code.len();
        self.code.push(data);
        CodeIdx(idx)
    }

    pub fn push_constant(&mut self, data: Value) -> ConstantIdx {
        let idx = self.constants.len();
        self.constants.push(data);
        ConstantIdx(idx)
    }

    pub fn constant(&self, idx: ConstantIdx) -> &Value {
        &self.constants[idx.0]
    }

    // Span tracking implementation

    fn push_span(&mut self, span: codemap::Span) {
        match self.spans.last_mut() {
            // We do not need to insert the same span again, as this
            // instruction was compiled from the same span as the last
            // one.
            Some(last) if last.span == span => last.count += 1,

            // In all other cases, this is a new source span.
            _ => self.spans.push(SourceSpan { span, count: 1 }),
        }
    }

    /// Retrieve the [codemap::Span] from which the instruction at
    /// `offset` was compiled.
    pub fn get_span(&self, offset: CodeIdx) -> codemap::Span {
        let mut pos = 0;

        for span in &self.spans {
            pos += span.count;
            if pos > offset.0 {
                return span.span;
            }
        }

        panic!("compiler error: chunk missing span for offset {}", offset.0);
    }
}
