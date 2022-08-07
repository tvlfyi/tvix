//! This module implements the instruction set running on the abstract
//! machine implemented by tvix.

#[derive(Clone, Copy, Debug)]
pub struct ConstantIdx(pub usize);

#[derive(Clone, Copy, Debug)]
pub struct CodeIdx(pub usize);

#[derive(Clone, Copy, Debug)]
pub enum OpCode {
    // Push a constant onto the stack.
    OpConstant(ConstantIdx),

    // Push a literal value.
    OpNull,
    OpTrue,
    OpFalse,
}
