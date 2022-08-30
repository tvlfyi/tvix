//! Implements methods for disassembling and printing a representation
//! of compiled code, as well as tracing the runtime stack during
//! execution.
use std::io::{Stderr, Write};
use tabwriter::TabWriter;

use crate::chunk::Chunk;
use crate::opcode::OpCode;
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
    write!(tw, "{:0width$}\t ", width = width).ok();

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
