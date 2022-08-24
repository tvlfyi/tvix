mod builtins;
mod chunk;
mod compiler;
mod errors;
mod eval;
mod opcode;
mod value;
mod vm;
mod warnings;

#[cfg(feature = "disassembler")]
mod disassembler;

#[cfg(test)]
mod tests;

pub use crate::errors::EvalResult;
pub use crate::eval::interpret;
pub use crate::value::Value;
