//! Implements methods for disassembling and printing a representation
//! of compiled code, as well as tracing the runtime stack during
//! execution.
use std::io::{Stderr, Write};
use tabwriter::TabWriter;

use crate::opcode::OpCode;
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
