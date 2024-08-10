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

/// Index of a value in the runtime stack.  This is an offset
/// *relative to* the VM value stack_base of the CallFrame
/// containing the opcode which contains this StackIdx.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd)]
pub struct StackIdx(pub usize);

/// Index of an upvalue within a closure's bound-variable upvalue
/// list.  This is an absolute index into the Upvalues of the
/// CallFrame containing the opcode which contains this UpvalueIdx.
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

/// Op represents all instructions in the Tvix abstract machine.
///
/// In documentation comments, stack positions are referred to by
/// indices written in `{}` as such, where required:
///
/// ```notrust
///                             --- top of the stack
///                            /
///                           v
///       [ ... | 3 | 2 | 1 | 0 ]
///                   ^
///                  /
/// 2 values deep ---
/// ```
///
/// Unless otherwise specified, operations leave their result at the
/// top of the stack.
#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
pub enum Op {
    /// Push a constant onto the stack.
    Constant,

    /// Discard the value on top of the stack.
    Pop,

    /// Invert the boolean at the top of the stack.
    Invert,

    /// Invert the sign of the number at the top of the stack.
    Negate,

    /// Sum up the two numbers at the top of the stack.
    Add,

    /// Subtract the number at {1} from the number at {2}.
    Sub,

    /// Multiply the two numbers at the top of the stack.
    Mul,

    /// Divide the two numbers at the top of the stack.
    Div,

    /// Check the two values at the top of the stack for Nix-equality.
    Equal,

    /// Check whether the value at {2} is less than {1}.
    Less,

    /// Check whether the value at {2} is less than or equal to {1}.
    LessOrEq,

    /// Check whether the value at {2} is greater than {1}.
    More,

    /// Check whether the value at {2} is greater than or equal to {1}.
    MoreOrEq,

    /// Jump forward in the bytecode specified by the number of
    /// instructions in its usize operand.
    Jump,

    /// Jump forward in the bytecode specified by the number of
    /// instructions in its usize operand, *if* the value at the top
    /// of the stack is `true`.
    JumpIfTrue,

    /// Jump forward in the bytecode specified by the number of
    /// instructions in its usize operand, *if* the value at the top
    /// of the stack is `false`.
    JumpIfFalse,

    /// Pop one stack item and jump forward in the bytecode
    /// specified by the number of instructions in its usize
    /// operand, *if* the value at the top of the stack is a
    /// Value::Catchable.
    JumpIfCatchable,

    /// Jump forward in the bytecode specified by the number of
    /// instructions in its usize operand, *if* the value at the top
    /// of the stack is the internal value representing a missing
    /// attribute set key.
    JumpIfNotFound,

    /// Jump forward in the bytecode specified by the number of
    /// instructions in its usize operand, *if* the value at the top
    /// of the stack is *not* the internal value requesting a
    /// stack value finalisation.
    JumpIfNoFinaliseRequest,

    /// Construct an attribute set from the given number of key-value pairs on
    /// the top of the stack. The operand gives the count of *pairs*, not the
    /// number of *stack values* - the actual number of values popped off the
    /// stack will be twice the argument to this op.
    Attrs,

    /// Merge the attribute set at {2} into the attribute set at {1},
    /// and leave the new set at the top of the stack.
    AttrsUpdate,

    /// Select the attribute with the name at {1} from the set at {2}.
    AttrsSelect,

    /// Select the attribute with the name at {1} from the set at {2}, but leave
    /// a `Value::AttrNotFound` in the stack instead of failing if it is
    /// missing.
    AttrsTrySelect,

    /// Check for the presence of the attribute with the name at {1} in the set
    /// at {2}.
    HasAttr,

    /// Throw an error if the attribute set at the top of the stack has any attributes
    /// other than those listed in the formals of the current lambda
    ///
    /// Panics if the current frame is not a lambda with formals
    ValidateClosedFormals,

    /// Push a value onto the runtime `with`-stack to enable dynamic identifier
    /// resolution. The absolute stack index of the value is supplied as a usize
    /// operand.
    PushWith,

    /// Pop the last runtime `with`-stack element.
    PopWith,

    /// Dynamically resolve an identifier with the name at {1} from the runtime
    /// `with`-stack.
    ResolveWith,

    // Lists
    /// Construct a list from the given number of values at the top of the
    /// stack.
    List,

    /// Concatenate the lists at {2} and {1}.
    Concat,

    // Strings
    /// Interpolate the given number of string fragments into a single string.
    Interpolate,

    /// Force the Value on the stack and coerce it to a string
    CoerceToString,

    // Paths
    /// Attempt to resolve the Value on the stack using the configured [`NixSearchPath`][]
    ///
    /// [`NixSearchPath`]: crate::nix_search_path::NixSearchPath
    FindFile,

    /// Attempt to resolve a path literal relative to the home dir
    ResolveHomePath,

    // Type assertion operators
    /// Assert that the value at {1} is a boolean, and fail with a runtime error
    /// otherwise.
    AssertBool,
    AssertAttrs,

    /// Access local identifiers with statically known positions.
    GetLocal,

    /// Close scopes while leaving their expression value around.
    CloseScope,

    /// Return an error indicating that an `assert` failed
    AssertFail,

    // Lambdas & closures
    /// Call the value at {1} in a new VM callframe
    Call,

    /// Retrieve the upvalue at the given index from the closure or thunk
    /// currently under evaluation.
    GetUpvalue,

    /// Construct a closure which has upvalues but no self-references
    Closure,

    /// Construct a closure which has self-references (direct or via upvalues)
    ThunkClosure,

    /// Construct a suspended thunk, used to delay a computation for laziness.
    ThunkSuspended,

    /// Force the value at {1} until it is a `Thunk::Evaluated`.
    Force,

    /// Finalise initialisation of the upvalues of the value in the given stack
    /// index (which must be a Value::Thunk) after the scope is fully bound.
    Finalise,

    /// Final instruction emitted in a chunk. Does not have an
    /// inherent effect, but can simplify VM logic as a marker in some
    /// cases.
    ///
    /// Can be thought of as "returning" the value to the parent
    /// frame, hence the name.
    Return,

    /// Sentinel value to signal invalid bytecode. This MUST always be the last
    /// value in the enum. Do not move it!
    Invalid,
}

const _ASSERT_SMALL_OP: () = assert!(std::mem::size_of::<Op>() == 1);

impl From<u8> for Op {
    fn from(num: u8) -> Self {
        if num >= Self::Invalid as u8 {
            return Self::Invalid;
        }

        // SAFETY: As long as `Invalid` remains the last variant of the enum,
        // and as long as variant values are not specified manually, this
        // conversion is safe.
        unsafe { std::mem::transmute(num) }
    }
}

pub enum OpArg {
    None,
    Uvarint,
    Fixed,
    Custom,
}

impl Op {
    pub fn arg_type(&self) -> OpArg {
        match self {
            Op::Constant
            | Op::Attrs
            | Op::PushWith
            | Op::List
            | Op::Interpolate
            | Op::GetLocal
            | Op::CloseScope
            | Op::GetUpvalue
            | Op::Finalise => OpArg::Uvarint,

            Op::Jump
            | Op::JumpIfTrue
            | Op::JumpIfFalse
            | Op::JumpIfCatchable
            | Op::JumpIfNotFound
            | Op::JumpIfNoFinaliseRequest => OpArg::Fixed,

            Op::CoerceToString | Op::Closure | Op::ThunkClosure | Op::ThunkSuspended => {
                OpArg::Custom
            }
            _ => OpArg::None,
        }
    }
}

/// Position is used to represent where to capture an upvalue from.
#[derive(Clone, Copy)]
pub struct Position(pub u64);

impl Position {
    pub fn stack_index(idx: StackIdx) -> Self {
        Position((idx.0 as u64) << 2)
    }

    pub fn deferred_local(idx: StackIdx) -> Self {
        Position(((idx.0 as u64) << 2) | 1)
    }

    pub fn upvalue_index(idx: UpvalueIdx) -> Self {
        Position(((idx.0 as u64) << 2) | 2)
    }

    pub fn runtime_stack_index(&self) -> Option<StackIdx> {
        if (self.0 & 0b11) == 0 {
            return Some(StackIdx((self.0 >> 2) as usize));
        }

        None
    }

    pub fn runtime_deferred_local(&self) -> Option<StackIdx> {
        if (self.0 & 0b11) == 1 {
            return Some(StackIdx((self.0 >> 2) as usize));
        }

        None
    }

    pub fn runtime_upvalue_index(&self) -> Option<UpvalueIdx> {
        if (self.0 & 0b11) == 2 {
            return Some(UpvalueIdx((self.0 >> 2) as usize));
        }

        None
    }
}

#[cfg(test)]
mod position_tests {
    use super::Position; // he-he
    use super::{StackIdx, UpvalueIdx};

    #[test]
    fn test_stack_index_position() {
        let idx = StackIdx(42);
        let pos = Position::stack_index(idx);
        let result = pos.runtime_stack_index();

        assert_eq!(result, Some(idx));
        assert_eq!(pos.runtime_deferred_local(), None);
        assert_eq!(pos.runtime_upvalue_index(), None);
    }

    #[test]
    fn test_deferred_local_position() {
        let idx = StackIdx(42);
        let pos = Position::deferred_local(idx);
        let result = pos.runtime_deferred_local();

        assert_eq!(result, Some(idx));
        assert_eq!(pos.runtime_stack_index(), None);
        assert_eq!(pos.runtime_upvalue_index(), None);
    }

    #[test]
    fn test_upvalue_index_position() {
        let idx = UpvalueIdx(42);
        let pos = Position::upvalue_index(idx);
        let result = pos.runtime_upvalue_index();

        assert_eq!(result, Some(idx));
        assert_eq!(pos.runtime_stack_index(), None);
        assert_eq!(pos.runtime_deferred_local(), None);
    }
}
