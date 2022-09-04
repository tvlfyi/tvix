mod builtins;
mod chunk;
mod compiler;
mod disassembler;
mod errors;
mod eval;
mod observer;
mod opcode;
mod upvalues;
mod value;
mod vm;
mod warnings;

#[cfg(test)]
mod tests;

pub use crate::errors::EvalResult;
pub use crate::eval::interpret;
pub use crate::value::Value;
