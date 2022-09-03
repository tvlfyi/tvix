//! Implements methods for disassembling and printing a representation
//! of compiled code, as well as tracing the runtime stack during
//! execution.
use std::io::{Stderr, Write};
use tabwriter::TabWriter;

use crate::chunk::Chunk;
use crate::opcode::{CodeIdx, OpCode};
use crate::value::Value;

/// Helper struct to trace runtime values and automatically flush the
/// output after the value is dropped (i.e. in both success and
/// failure exits from the VM).
pub struct Tracer(TabWriter<Stderr>);

impl Default for Tracer {
    fn default() -> Self {
        Tracer(TabWriter::new(std::io::stderr()))
    }
}

impl Tracer {
    pub fn trace(&mut self, op: &OpCode, ip: usize, stack: &[Value]) {
        let _ = write!(&mut self.0, "{:04} {:?}\t[ ", ip, op);

        for val in stack {
            let _ = write!(&mut self.0, "{} ", val);
        }

        let _ = writeln!(&mut self.0, "]");
    }

    pub fn literal(&mut self, line: &str) {
        let _ = writeln!(&mut self.0, "{}", line);
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        let _ = self.0.flush();
    }
}

fn disassemble_op(tw: &mut TabWriter<Stderr>, chunk: &Chunk, width: usize, offset: usize) {
    let _ = write!(tw, "{:0width$}\t ", offset, width = width);

    let span = chunk.get_span(CodeIdx(offset));

    if offset > 0 && chunk.get_span(CodeIdx(offset - 1)) == span {
        write!(tw, "   |\t").unwrap();
    } else {
        let loc = chunk.codemap.look_up_span(span);
        write!(tw, "{:4}\t", loc.begin.line + 1).unwrap();
    }

    let _ = match chunk.code[offset] {
        OpCode::OpConstant(idx) => writeln!(tw, "OpConstant({})", chunk.constant(idx)),
        op => writeln!(tw, "{:?}", op),
    };
}

/// Disassemble a chunk of code, printing out the operations in a
/// reasonable, human-readable format.
pub fn disassemble_chunk(chunk: &Chunk) {
    let mut tw = TabWriter::new(std::io::stderr());

    let _ = writeln!(
        &mut tw,
        "=== compiled bytecode ({} operations) ===",
        chunk.code.len()
    );

    let width = format!("{}", chunk.code.len()).len();
    for (idx, _) in chunk.code.iter().enumerate() {
        disassemble_op(&mut tw, chunk, width, idx);
    }

    let _ = tw.flush();
}
