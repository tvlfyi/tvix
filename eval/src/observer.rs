//! Implements traits for things that wish to observe internal state
//! changes of tvix-eval.
//!
//! This can be used to gain insights from compilation, to trace the
//! runtime, and so on.
//!
//! All methods are optional, that is, observers can implement only
/// what they are interested in observing.
use std::io::Write;
use std::rc::Rc;
use tabwriter::TabWriter;

use crate::chunk::Chunk;
use crate::opcode::{CodeIdx, OpCode};
use crate::value::Lambda;
use crate::SourceCode;
use crate::Value;

/// Implemented by types that wish to observe internal happenings of
/// the Tvix compiler.
pub trait CompilerObserver {
    /// Called when the compiler finishes compilation of the top-level
    /// of an expression (usually the root Nix expression of a file).
    fn observe_compiled_toplevel(&mut self, _: &Rc<Lambda>) {}

    /// Called when the compiler finishes compilation of a
    /// user-defined function.
    ///
    /// Note that in Nix there are only single argument functions, so
    /// in an expression like `a: b: c: ...` this method will be
    /// called three times.
    fn observe_compiled_lambda(&mut self, _: &Rc<Lambda>) {}

    /// Called when the compiler finishes compilation of a thunk.
    fn observe_compiled_thunk(&mut self, _: &Rc<Lambda>) {}
}

/// Implemented by types that wish to observe internal happenings of
/// the Tvix virtual machine at runtime.
pub trait RuntimeObserver {
    /// Called when the runtime enters a new call frame.
    fn observe_enter_frame(&mut self, _arg_count: usize, _: &Rc<Lambda>, _call_depth: usize) {}

    /// Called when the runtime exits a call frame.
    fn observe_exit_frame(&mut self, _frame_at: usize, _stack: &[Value]) {}

    /// Called when the runtime replaces the current call frame for a
    /// tail call.
    fn observe_tail_call(&mut self, _frame_at: usize, _: &Rc<Lambda>) {}

    /// Called when the runtime enters a builtin.
    fn observe_enter_builtin(&mut self, _name: &'static str) {}

    /// Called when the runtime exits a builtin.
    fn observe_exit_builtin(&mut self, _name: &'static str, _stack: &[Value]) {}

    /// Called when the runtime *begins* executing an instruction. The
    /// provided stack is the state at the beginning of the operation.
    fn observe_execute_op(&mut self, _ip: CodeIdx, _: &OpCode, _: &[Value]) {}
}

#[derive(Default)]
pub struct NoOpObserver {}

impl CompilerObserver for NoOpObserver {}
impl RuntimeObserver for NoOpObserver {}

/// An observer that prints disassembled chunk information to its
/// internal writer whenwever the compiler emits a toplevel function,
/// closure or thunk.
pub struct DisassemblingObserver<W: Write> {
    source: SourceCode,
    writer: TabWriter<W>,
}

impl<W: Write> DisassemblingObserver<W> {
    pub fn new(source: SourceCode, writer: W) -> Self {
        Self {
            source,
            writer: TabWriter::new(writer),
        }
    }

    fn lambda_header(&mut self, kind: &str, lambda: &Rc<Lambda>) {
        let _ = writeln!(
            &mut self.writer,
            "=== compiled {} @ {:p} ({} ops) ===",
            kind,
            *lambda,
            lambda.chunk.code.len()
        );
    }

    fn disassemble_chunk(&mut self, chunk: &Chunk) {
        // calculate width of the widest address in the chunk
        let width = format!("{:#x}", chunk.code.len() - 1).len();

        for (idx, _) in chunk.code.iter().enumerate() {
            let _ = chunk.disassemble_op(&mut self.writer, &self.source, width, CodeIdx(idx));
        }
    }
}

impl<W: Write> CompilerObserver for DisassemblingObserver<W> {
    fn observe_compiled_toplevel(&mut self, lambda: &Rc<Lambda>) {
        self.lambda_header("toplevel", lambda);
        self.disassemble_chunk(&lambda.chunk);
        let _ = self.writer.flush();
    }

    fn observe_compiled_lambda(&mut self, lambda: &Rc<Lambda>) {
        self.lambda_header("lambda", lambda);
        self.disassemble_chunk(&lambda.chunk);
        let _ = self.writer.flush();
    }

    fn observe_compiled_thunk(&mut self, lambda: &Rc<Lambda>) {
        self.lambda_header("thunk", lambda);
        self.disassemble_chunk(&lambda.chunk);
        let _ = self.writer.flush();
    }
}

/// An observer that collects a textual representation of an entire
/// runtime execution.
pub struct TracingObserver<W: Write> {
    writer: TabWriter<W>,
}

impl<W: Write> TracingObserver<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: TabWriter::new(writer),
        }
    }
}

impl<W: Write> RuntimeObserver for TracingObserver<W> {
    fn observe_enter_frame(&mut self, arg_count: usize, lambda: &Rc<Lambda>, call_depth: usize) {
        let _ = writeln!(
            &mut self.writer,
            "=== entering {} frame[{}] @ {:p} ===",
            if arg_count == 0 { "thunk" } else { "closure" },
            call_depth,
            lambda,
        );
    }

    fn observe_exit_frame(&mut self, frame_at: usize, stack: &[Value]) {
        let _ = write!(&mut self.writer, "=== exiting frame {} ===\t[ ", frame_at);

        for val in stack {
            let _ = write!(&mut self.writer, "{} ", val);
        }

        let _ = writeln!(&mut self.writer, "]");
    }

    fn observe_enter_builtin(&mut self, name: &'static str) {
        let _ = writeln!(&mut self.writer, "=== entering builtin {} ===", name);
    }

    fn observe_exit_builtin(&mut self, name: &'static str, stack: &[Value]) {
        let _ = write!(&mut self.writer, "=== exiting builtin {} ===\t[ ", name);

        for val in stack {
            let _ = write!(&mut self.writer, "{} ", val);
        }

        let _ = writeln!(&mut self.writer, "]");
    }

    fn observe_tail_call(&mut self, frame_at: usize, lambda: &Rc<Lambda>) {
        let _ = writeln!(
            &mut self.writer,
            "=== tail-calling {:p} in frame[{}] ===",
            lambda, frame_at
        );
    }

    fn observe_execute_op(&mut self, ip: CodeIdx, op: &OpCode, stack: &[Value]) {
        let _ = write!(&mut self.writer, "{:04} {:?}\t[ ", ip.0, op);

        for val in stack {
            let _ = write!(&mut self.writer, "{} ", val);
        }

        let _ = writeln!(&mut self.writer, "]");
    }
}

impl<W: Write> Drop for TracingObserver<W> {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}
