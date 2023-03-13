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
use crate::generators::VMRequest;
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
    fn observe_enter_call_frame(&mut self, _arg_count: usize, _: &Rc<Lambda>, _call_depth: usize) {}

    /// Called when the runtime exits a call frame.
    fn observe_exit_call_frame(&mut self, _frame_at: usize, _stack: &[Value]) {}

    /// Called when the runtime suspends a call frame.
    fn observe_suspend_call_frame(&mut self, _frame_at: usize, _stack: &[Value]) {}

    /// Called when the runtime enters a generator frame.
    fn observe_enter_generator(&mut self, _frame_at: usize, _name: &str, _stack: &[Value]) {}

    /// Called when the runtime exits a generator frame.
    fn observe_exit_generator(&mut self, _frame_at: usize, _name: &str, _stack: &[Value]) {}

    /// Called when the runtime suspends a generator frame.
    fn observe_suspend_generator(&mut self, _frame_at: usize, _name: &str, _stack: &[Value]) {}

    /// Called when a generator requests an action from the VM.
    fn observe_generator_request(&mut self, _name: &str, _msg: &VMRequest) {}

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

    fn write_value(&mut self, val: &Value) {
        let _ = match val {
            // Potentially large types which we only want to print
            // the type of (and avoid recursing).
            Value::List(l) => write!(&mut self.writer, "list[{}] ", l.len()),
            Value::Attrs(a) => write!(&mut self.writer, "attrs[{}] ", a.len()),
            Value::Thunk(t) if t.is_evaluated() => Ok(self.write_value(&t.value())),

            // For other value types, defer to the standard value printer.
            _ => write!(&mut self.writer, "{} ", val),
        };
    }

    fn write_stack(&mut self, stack: &[Value]) {
        let _ = write!(&mut self.writer, "[ ");

        // Print out a maximum of 6 values from the top of the stack,
        // before abbreviating it to `...`.
        for (i, val) in stack.iter().rev().enumerate() {
            if i == 6 {
                let _ = write!(&mut self.writer, "...");
                break;
            }

            self.write_value(&val);
        }

        let _ = writeln!(&mut self.writer, "]");
    }
}

impl<W: Write> RuntimeObserver for TracingObserver<W> {
    fn observe_enter_call_frame(
        &mut self,
        arg_count: usize,
        lambda: &Rc<Lambda>,
        call_depth: usize,
    ) {
        let _ = write!(&mut self.writer, "=== entering ");

        let _ = if arg_count == 0 {
            write!(&mut self.writer, "thunk ")
        } else {
            write!(&mut self.writer, "closure ")
        };

        if let Some(name) = &lambda.name {
            let _ = write!(&mut self.writer, "'{}' ", name);
        }

        let _ = writeln!(
            &mut self.writer,
            "in frame[{}] @ {:p} ===",
            call_depth, *lambda
        );
    }

    /// Called when the runtime exits a call frame.
    fn observe_exit_call_frame(&mut self, frame_at: usize, stack: &[Value]) {
        let _ = write!(&mut self.writer, "=== exiting frame {} ===\t ", frame_at);
        self.write_stack(stack);
    }

    fn observe_suspend_call_frame(&mut self, frame_at: usize, stack: &[Value]) {
        let _ = write!(&mut self.writer, "=== suspending frame {} ===\t", frame_at);

        self.write_stack(stack);
    }

    fn observe_enter_generator(&mut self, frame_at: usize, name: &str, stack: &[Value]) {
        let _ = write!(
            &mut self.writer,
            "=== entering generator frame '{}' [{}] ===\t",
            name, frame_at,
        );

        self.write_stack(stack);
    }

    fn observe_exit_generator(&mut self, frame_at: usize, name: &str, stack: &[Value]) {
        let _ = write!(
            &mut self.writer,
            "=== exiting generator '{}' [{}] ===\t",
            name, frame_at
        );

        self.write_stack(stack);
    }

    fn observe_suspend_generator(&mut self, frame_at: usize, name: &str, stack: &[Value]) {
        let _ = write!(
            &mut self.writer,
            "=== suspending generator '{}' [{}] ===\t",
            name, frame_at
        );

        self.write_stack(stack);
    }

    fn observe_generator_request(&mut self, name: &str, msg: &VMRequest) {
        let _ = writeln!(
            &mut self.writer,
            "=== generator '{}' requested {} ===",
            name, msg
        );
    }

    fn observe_enter_builtin(&mut self, name: &'static str) {
        let _ = writeln!(&mut self.writer, "=== entering builtin {} ===", name);
    }

    fn observe_exit_builtin(&mut self, name: &'static str, stack: &[Value]) {
        let _ = write!(&mut self.writer, "=== exiting builtin {} ===\t", name);
        self.write_stack(stack);
    }

    fn observe_tail_call(&mut self, frame_at: usize, lambda: &Rc<Lambda>) {
        let _ = writeln!(
            &mut self.writer,
            "=== tail-calling {:p} in frame[{}] ===",
            *lambda, frame_at
        );
    }

    fn observe_execute_op(&mut self, ip: CodeIdx, op: &OpCode, stack: &[Value]) {
        let _ = write!(&mut self.writer, "{:04} {:?}\t", ip.0, op);
        self.write_stack(stack);
    }
}

impl<W: Write> Drop for TracingObserver<W> {
    fn drop(&mut self) {
        let _ = self.writer.flush();
    }
}
