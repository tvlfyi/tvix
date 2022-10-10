//! This module implements the instruction set running on the abstract
//! machine implemented by tvix.

use std::ops::{AddAssign, Sub};

/// Index of a constant in the current code chunk.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstantIdx(pub usize);

/// Index of an instruction in the current code chunk.
#[repr(transparent)]
#[derive(Clone, Copy, Debug)]
pub struct CodeIdx(pub usize);

impl AddAssign<usize> for CodeIdx {
    fn add_assign(&mut self, rhs: usize) {
        *self = CodeIdx(self.0 + rhs)
    }
}

impl Sub<usize> for CodeIdx {
    type Output = Self;

    fn sub(self, rhs: usize) -> Self::Output {
        CodeIdx(self.0 - rhs)
    }
}

/// Index of a value in the runtime stack.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub struct StackIdx(pub usize);

/// Index of an upvalue within a closure's upvalue list.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpvalueIdx(pub usize);

/// Offset by which an instruction pointer should change in a jump.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JumpOffset(pub usize);

/// Provided count for an instruction (could represent e.g. a number
/// of elements).
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Count(pub usize);

#[warn(variant_size_differences)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCode {
    /// Push a constant onto the stack.
    OpConstant(ConstantIdx),

    /// Discard a value from the stack.
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
    OpJump(JumpOffset),
    OpJumpIfTrue(JumpOffset),
    OpJumpIfFalse(JumpOffset),
    OpJumpIfNotFound(JumpOffset),

    // Attribute sets
    /// Construct an attribute set from the given number of key-value pairs on the top of the stack
    ///
    /// Note that this takes the count of *pairs*, not the number of *stack values* - the actual
    /// number of values popped off the stack will be twice the argument to this op
    OpAttrs(Count),
    OpAttrsUpdate,
    OpAttrsSelect,
    OpAttrsTrySelect,
    OpHasAttr,

    // `with`-handling
    OpPushWith(StackIdx),
    OpPopWith,
    OpResolveWith,
    OpResolveWithOrUpvalue(UpvalueIdx),

    // Lists
    OpList(Count),
    OpConcat,

    // Strings
    OpInterpolate(Count),
    /// Force the Value on the stack and coerce it to a string, always using
    /// `CoercionKind::Weak`.
    OpCoerceToString,

    // Paths
    /// Attempt to resolve the Value on the stack using the configured [`NixPath`][]
    ///
    /// [`NixPath`]: crate::nix_path::NixPath
    OpFindFile,

    // Type assertion operators
    OpAssertBool,

    /// Access local identifiers with statically known positions.
    OpGetLocal(StackIdx),

    /// Close scopes while leaving their expression value around.
    OpCloseScope(Count), // number of locals to pop

    /// Return an error indicating that an `assert` failed
    OpAssertFail,

    // Lambdas & closures
    OpCall,
    OpTailCall,
    OpGetUpvalue(UpvalueIdx),
    OpClosure(ConstantIdx),

    // Thunks
    OpThunk(ConstantIdx),
    OpForce,

    /// Finalise initialisation of the upvalues of the value in the
    /// given stack index after the scope is fully bound.
    OpFinalise(StackIdx),

    // The closure and thunk creation instructions have a variable
    // number of arguments to the instruction, which is represented
    // here by making their data part of the opcodes.
    //
    // The VM skips over these by advancing the instruction pointer
    // according to the count.
    DataLocalIdx(StackIdx),
    DataDeferredLocal(StackIdx),
    DataUpvalueIdx(UpvalueIdx),
    DataCaptureWith,
}
