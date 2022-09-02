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

impl Tracer {
    pub fn new() -> Self {
        let mut tw = TabWriter::new(std::io::stderr());
        write!(&mut tw, "=== runtime trace ===\n").ok();
        Tracer(tw)
    }

    pub fn trace(&mut self, op: &OpCode, ip: usize, stack: &[Value]) {
        write!(&mut self.0, "{:04} {:?}\t[ ", ip, op).ok();

        for val in stack {
            write!(&mut self.0, "{} ", val).ok();
        }

        write!(&mut self.0, "]\n").ok();
    }
}

impl Drop for Tracer {
    fn drop(&mut self) {
        self.0.flush().ok();
    }
}

fn disassemble_op(tw: &mut TabWriter<Stderr>, chunk: &Chunk, width: usize, offset: usize) {
    write!(tw, "{:0width$}\t ", offset, width = width).ok();

    let span = chunk.get_span(CodeIdx(offset));

    if offset > 0 && chunk.get_span(CodeIdx(offset - 1)) == span {
        write!(tw, "   |\t").unwrap();
    } else {
        let loc = chunk.codemap.look_up_span(span);
        write!(tw, "{:4}\t", loc.begin.line + 1).unwrap();
    }

    match chunk.code[offset] {
        OpCode::OpConstant(idx) => write!(tw, "OpConstant({})\n", chunk.constant(idx)).ok(),

        op => write!(tw, "{:?}\n", op).ok(),
    };
}

/// Disassemble a chunk of code, printing out the operations in a
/// reasonable, human-readable format.
pub fn disassemble_chunk(chunk: &Chunk) {
    let mut tw = TabWriter::new(std::io::stderr());

    write!(
        &mut tw,
        "=== compiled bytecode ({} operations) ===\n",
        chunk.code.len()
    )
    .ok();

    let width = format!("{}", chunk.code.len()).len();
    for (idx, _) in chunk.code.iter().enumerate() {
        disassemble_op(&mut tw, chunk, width, idx);
    }

    tw.flush().ok();
}
