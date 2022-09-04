//! Implements methods for disassembling and printing a representation
//! of compiled code, as well as tracing the runtime stack during
//! execution.
use codemap::CodeMap;
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

pub fn disassemble_op<W: Write>(
    tw: &mut W,
    codemap: &CodeMap,
    chunk: &Chunk,
    width: usize,
    idx: CodeIdx,
) {
    let _ = write!(tw, "{:#width$x}\t ", idx.0, width = width);

    // Print continuation character if the previous operation was at
    // the same line, otherwise print the line.
    let line = chunk.get_line(codemap, idx);
    if idx.0 > 0 && chunk.get_line(codemap, CodeIdx(idx.0 - 1)) == line {
        write!(tw, "   |\t").unwrap();
    } else {
        write!(tw, "{:4}\t", line).unwrap();
    }

    let _ = match chunk[idx] {
        OpCode::OpConstant(idx) => writeln!(tw, "OpConstant({}@{})", chunk[idx], idx.0),
        op => writeln!(tw, "{:?}", op),
    };
}
