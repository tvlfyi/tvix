use crate::opcode::{CodeIdx, ConstantIdx, Op, OpArg};
use crate::value::Value;
use crate::{CoercionKind, SourceCode};
use std::io::Write;

/// Maximum size of a u64 encoded in the vu128 varint encoding.
const U64_VARINT_SIZE: usize = 9;

/// Represents a source location from which one or more operations
/// were compiled.
///
/// The span itself is an index into a [codemap::CodeMap], and the
/// structure tracks the number of operations that were yielded from
/// the same span.
///
/// At error reporting time, it becomes possible to either just fetch
/// the textual representation of that span from the codemap, or to
/// even re-parse the AST using rnix to create more semantically
/// interesting errors.
#[derive(Clone, Debug, PartialEq)]
struct SourceSpan {
    /// Span into the [codemap::CodeMap].
    span: codemap::Span,

    /// Index of the first operation covered by this span.
    start: usize,
}

/// A chunk is a representation of a sequence of bytecode
/// instructions, associated constants and additional metadata as
/// emitted by the compiler.
#[derive(Debug, Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<Value>,
    spans: Vec<SourceSpan>,

    /// Index of the last operation (i.e. not data) written to the code vector.
    /// Some operations (e.g. jump patching) need to know this.
    last_op: usize,
}

impl Chunk {
    pub fn push_op(&mut self, op: Op, span: codemap::Span) -> usize {
        self.last_op = self.code.len();
        self.code.push(op as u8);
        self.push_span(span, self.last_op);
        self.last_op
    }

    pub fn push_uvarint(&mut self, data: u64) {
        let mut encoded = [0u8; U64_VARINT_SIZE];
        let bytes_written = vu128::encode_u64(&mut encoded, data);
        self.code.extend_from_slice(&encoded[..bytes_written]);
    }

    pub fn read_uvarint(&self, idx: usize) -> (u64, usize) {
        debug_assert!(
            idx < self.code.len(),
            "invalid bytecode (missing varint operand)",
        );

        if self.code.len() - idx >= U64_VARINT_SIZE {
            vu128::decode_u64(
                &self.code[idx..idx + U64_VARINT_SIZE]
                    .try_into()
                    .expect("size statically checked"),
            )
        } else {
            let mut tmp = [0u8; U64_VARINT_SIZE];
            tmp[..self.code.len() - idx].copy_from_slice(&self.code[idx..]);
            vu128::decode_u64(&tmp)
        }
    }

    pub fn push_u16(&mut self, data: u16) {
        self.code.extend_from_slice(&data.to_le_bytes())
    }

    /// Patches the argument to the jump operand of the jump at the given index
    /// to point to the *next* instruction that will be emitted.
    pub fn patch_jump(&mut self, idx: usize) {
        let offset = (self.code.len() - idx - /* arg idx = */ 1 - /* jump arg size = */ 2) as u16;
        self.code[idx + 1..idx + 3].copy_from_slice(&offset.to_le_bytes())
    }

    pub fn read_u16(&self, idx: usize) -> u16 {
        if idx + 2 > self.code.len() {
            panic!("Tvix bug: invalid bytecode (expected u16 operand not found)")
        }

        let byte_array: &[u8; 2] = &self.code[idx..idx + 2]
            .try_into()
            .expect("fixed-size slice can not fail to convert to array");

        u16::from_le_bytes(*byte_array)
    }

    /// Get the first span of a chunk, no questions asked.
    pub fn first_span(&self) -> codemap::Span {
        self.spans[0].span
    }

    /// Return the last op in the chunk together with its index, if any.
    pub fn last_op(&self) -> Option<(Op, usize)> {
        if self.code.is_empty() {
            return None;
        }

        Some((self.code[self.last_op].into(), self.last_op))
    }

    pub fn push_constant(&mut self, data: Value) -> ConstantIdx {
        let idx = self.constants.len();
        self.constants.push(data);
        ConstantIdx(idx)
    }

    /// Return a reference to the constant at the given [`ConstantIdx`]
    pub fn get_constant(&self, constant: ConstantIdx) -> Option<&Value> {
        self.constants.get(constant.0)
    }

    fn push_span(&mut self, span: codemap::Span, start: usize) {
        match self.spans.last_mut() {
            // We do not need to insert the same span again, as this
            // instruction was compiled from the same span as the last
            // one.
            Some(last) if last.span == span => {}

            // In all other cases, this is a new source span.
            _ => self.spans.push(SourceSpan { span, start }),
        }
    }

    /// Retrieve the [codemap::Span] from which the instruction at
    /// `offset` was compiled.
    pub fn get_span(&self, offset: CodeIdx) -> codemap::Span {
        let position = self
            .spans
            .binary_search_by(|span| span.start.cmp(&offset.0));

        let span = match position {
            Ok(index) => &self.spans[index],
            Err(index) => {
                if index == 0 {
                    &self.spans[0]
                } else {
                    &self.spans[index - 1]
                }
            }
        };

        span.span
    }

    /// Write the disassembler representation of the operation at
    /// `idx` to the specified writer, and return how many bytes in the code to
    /// skip for the next op.
    pub fn disassemble_op<W: Write>(
        &self,
        writer: &mut W,
        source: &SourceCode,
        width: usize,
        idx: CodeIdx,
    ) -> Result<usize, std::io::Error> {
        write!(writer, "{:#width$x}\t ", idx.0, width = width)?;

        // Print continuation character if the previous operation was at
        // the same line, otherwise print the line.
        let line = source.get_line(self.get_span(idx));
        if idx.0 > 0 && source.get_line(self.get_span(idx - 1)) == line {
            write!(writer, "   |\t")?;
        } else {
            write!(writer, "{:4}\t", line)?;
        }

        let _fmt_constant = |idx: ConstantIdx| match &self.constants[idx.0] {
            Value::Thunk(t) => t.debug_repr(),
            Value::Closure(c) => format!("closure({:p})", c.lambda),
            Value::Blueprint(b) => format!("blueprint({:p})", b),
            val => format!("{}", val),
        };

        let op: Op = self.code[idx.0].into();

        match op.arg_type() {
            OpArg::None => {
                writeln!(writer, "Op{:?}", op)?;
                Ok(1)
            }

            OpArg::Fixed => {
                let arg = self.read_u16(idx.0 + 1);
                writeln!(writer, "Op{:?}({})", op, arg)?;
                Ok(3)
            }

            OpArg::Uvarint => {
                let (arg, size) = self.read_uvarint(idx.0 + 1);
                writeln!(writer, "Op{:?}({})", op, arg)?;
                Ok(1 + size)
            }

            _ => match op {
                Op::CoerceToString => {
                    let kind: CoercionKind = self.code[idx.0 + 1].into();
                    writeln!(writer, "Op{:?}({:?})", op, kind)?;
                    Ok(2)
                }

                Op::Closure | Op::ThunkClosure | Op::ThunkSuspended => {
                    let mut cidx = idx.0 + 1;

                    let (bp_idx, size) = self.read_uvarint(cidx);
                    cidx += size;

                    let (packed_count, size) = self.read_uvarint(cidx);
                    cidx += size;

                    let captures_with = packed_count & 0b1 == 1;
                    let count = packed_count >> 1;

                    write!(writer, "Op{:?}(BP @ {}, ", op, bp_idx)?;
                    if captures_with {
                        write!(writer, "captures with, ")?;
                    }
                    writeln!(writer, "{} upvalues)", count)?;

                    for _ in 0..count {
                        let (_, size) = self.read_uvarint(cidx);
                        cidx += size;
                    }

                    Ok(cidx - idx.0)
                }
                _ => panic!("Tvix bug: don't know how to format argument for Op{:?}", op),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::dummy_span;

    // Note: These tests are about the functionality of the `Chunk` type, the
    // opcodes used below do *not* represent valid, executable Tvix code (and
    // don't need to).

    #[test]
    fn push_op() {
        let mut chunk = Chunk::default();
        let idx = chunk.push_op(Op::Add, dummy_span());
        assert_eq!(*chunk.code.last().unwrap(), Op::Add as u8);
        assert_eq!(chunk.code[idx], Op::Add as u8);
    }

    #[test]
    fn push_op_with_arg() {
        let mut chunk = Chunk::default();
        let mut idx = chunk.push_op(Op::Constant, dummy_span());
        chunk.push_uvarint(42);

        assert_eq!(chunk.code[idx], Op::Constant as u8);

        idx += 1;
        let (arg, size) = chunk.read_uvarint(idx);
        assert_eq!(idx + size, chunk.code.len());
        assert_eq!(arg, 42);
    }

    #[test]
    fn push_jump() {
        let mut chunk = Chunk::default();

        chunk.push_op(Op::Constant, dummy_span());
        chunk.push_uvarint(0);

        let idx = chunk.push_op(Op::Jump, dummy_span());
        chunk.push_u16(0);

        chunk.push_op(Op::Constant, dummy_span());
        chunk.push_uvarint(1);

        chunk.patch_jump(idx);
        chunk.push_op(Op::Return, dummy_span());

        #[rustfmt::skip]
        let expected: Vec<u8> = vec![
            Op::Constant as u8, 0,
            Op::Jump as u8, 2, 0,
            Op::Constant as u8, 1,
            Op::Return as u8,
        ];

        assert_eq!(chunk.code, expected);
    }
}
