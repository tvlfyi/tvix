mod builtins;
mod chunk;
mod compiler;
mod disassembler;
mod errors;
mod eval;
pub mod observer;
mod opcode;
mod upvalues;
mod value;
mod vm;
mod warnings;

#[cfg(test)]
mod tests;

// Re-export the public interface used by other crates.
pub use crate::builtins::global_builtins;
pub use crate::compiler::compile;
pub use crate::errors::EvalResult;
pub use crate::eval::interpret;
pub use crate::value::Value;
pub use crate::vm::run_lambda;
