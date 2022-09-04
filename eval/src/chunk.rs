use std::io::Write;
use std::ops::Index;

use codemap::CodeMap;

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

/// A chunk is a representation of a sequence of bytecode
/// instructions, associated constants and additional metadata as
/// emitted by the compiler.
#[derive(Clone, Debug, Default)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<Value>,
    spans: Vec<SourceSpan>,
}

impl Index<ConstantIdx> for Chunk {
    type Output = Value;

    fn index(&self, index: ConstantIdx) -> &Self::Output {
        &self.constants[index.0]
    }
}

impl Index<CodeIdx> for Chunk {
    type Output = OpCode;

    fn index(&self, index: CodeIdx) -> &Self::Output {
        &self.code[index.0]
    }
}

impl Chunk {
    pub fn push_op(&mut self, data: OpCode, span: codemap::Span) -> CodeIdx {
        let idx = self.code.len();
        self.code.push(data);
        self.push_span(span);
        CodeIdx(idx)
    }

    pub fn push_constant(&mut self, data: Value) -> ConstantIdx {
        let idx = self.constants.len();
        self.constants.push(data);
        ConstantIdx(idx)
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

    /// Retrieve the line from which the instruction at `offset` was
    /// compiled in the specified codemap.
    pub fn get_line(&self, codemap: &codemap::CodeMap, offset: CodeIdx) -> usize {
        let span = self.get_span(offset);
        // lines are 0-indexed in the codemap, but users probably want
        // real line numbers
        codemap.look_up_span(span).begin.line + 1
    }

    /// Write the disassembler representation of the operation at
    /// `idx` to the specified writer.
    pub fn disassemble_op<W: Write>(
        &self,
        writer: &mut W,
        codemap: &CodeMap,
        width: usize,
        idx: CodeIdx,
    ) -> Result<(), std::io::Error> {
        write!(writer, "{:#width$x}\t ", idx.0, width = width)?;

        // Print continuation character if the previous operation was at
        // the same line, otherwise print the line.
        let line = self.get_line(codemap, idx);
        if idx.0 > 0 && self.get_line(codemap, CodeIdx(idx.0 - 1)) == line {
            write!(writer, "   |\t")?;
        } else {
            write!(writer, "{:4}\t", line)?;
        }

        match self[idx] {
            OpCode::OpConstant(idx) => writeln!(writer, "OpConstant({}@{})", self[idx], idx.0),
            op => writeln!(writer, "{:?}", op),
        }?;

        Ok(())
    }
}
