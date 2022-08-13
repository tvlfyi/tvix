//! This module implements the instruction set running on the abstract
//! machine implemented by tvix.

#[derive(Clone, Copy, Debug)]
pub struct ConstantIdx(pub usize);

#[derive(Clone, Copy, Debug)]
pub struct CodeIdx(pub usize);

#[warn(variant_size_differences)]
#[derive(Clone, Copy, Debug)]
pub enum OpCode {
    // Push a constant onto the stack.
    OpConstant(ConstantIdx),

    // Discard a value from the stack.
    OpPop,

    // Push a literal value.
    OpNull,
    OpTrue,
    OpFalse,

    // Unary operators
    OpInvert,
    OpNegate,

    // Arithmetic binary operators
    OpAdd,
    OpSub,
    OpMul,
    OpDiv,

    // Comparison operators
    OpEqual,
    OpLess,
    OpLessOrEq,
    OpMore,
    OpMoreOrEq,

    // Logical operators & generic jumps
    OpJump(usize),
    OpJumpIfTrue(usize),
    OpJumpIfFalse(usize),
    OpJumpIfNotFound(usize),

    // Attribute sets
    OpAttrs(usize),
    OpAttrPath(usize),
    OpAttrsUpdate,
    OpAttrsSelect,
    OpAttrOrNotFound,
    OpAttrsIsSet,

    // Lists
    OpList(usize),
    OpConcat,

    // Strings
    OpInterpolate(usize),

    // Type assertion operators
    OpAssertBool,

    // Close scopes while leaving their expression value around.
    OpCloseScope(usize), // number of locals to pop
}
