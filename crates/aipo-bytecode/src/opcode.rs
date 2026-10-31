//! Instruction set opcodes and constant definitions for the Aipo VM.

use serde::{Deserialize, Serialize};

/// Opcodes for the Aipo Bytecode Virtual Machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum OpCode {
    /// Load constant from constant pool.
    Constant = 1,
    /// Push `none`.
    Nil,
    /// Push `true`.
    True,
    /// Push `false`.
    False,
    /// Pop top value.
    Pop,
    /// Duplicate top value.
    Dup,
    /// Get local variable by slot index.
    GetLocal,
    /// Set local variable by slot index.
    SetLocal,
    /// Get global variable by name pool index.
    GetGlobal,
    /// Set global variable by name pool index.
    SetGlobal,
    /// Binary addition `+`.
    Add,
    /// Binary subtraction `-`.
    Sub,
    /// Binary multiplication `*`.
    Mul,
    /// Binary floating-point division `/`.
    Div,
    /// Binary integer truncated division `div`.
    IntDiv,
    /// Binary modulo `%`.
    Mod,
    /// Unary numeric negation `-`.
    Neg,
    /// Logical NOT `not`.
    Not,
    /// Equality comparison `==`.
    Equal,
    /// Inequality comparison `!=`.
    NotEqual,
    /// Less than `<`.
    Less,
    /// Less than or equal `<=`.
    LessEqual,
    /// Greater than `>`.
    Greater,
    /// Greater than or equal `>=`.
    GreaterEqual,
    /// Unconditional relative jump.
    Jump,
    /// Relative jump if top of stack is false.
    JumpIfFalse,
    /// Call function with argument count.
    Call,
    /// Return from current frame.
    Return,
    /// Get field by name index.
    GetField,
    /// Set field by name index.
    SetField,
    /// Get collection item by index.
    GetIndex,
    /// Set collection item by index.
    SetIndex,
    /// Build list from top N items.
    BuildList,
    /// Build dictionary from top N key-value pairs.
    BuildDict,
    /// Build struct instance from type and field count.
    BuildStruct,
    /// Fail with recoverable error value.
    Fail,
    /// Failure fallback `or_else`.
    OrElse,
    /// Push exception recovery handler frame.
    PushHandler,
    /// Pop topmost exception recovery handler frame.
    PopHandler,
    /// Push a function value for a declared function: `u16` name index.
    MakeFunction,
    /// Push a closure: `u16` name index, `u16` upvalue count (values already pushed).
    MakeClosure,
    /// Load an upvalue of the current closure: `u16` upvalue index.
    GetUpvalue,
    /// Store top of stack into an upvalue: `u16` upvalue index.
    SetUpvalue,
    /// Build a half-open range value from two Ints.
    Range,
    /// Push the length of a String, List, or Dict.
    Len,
    /// Runtime type test of a value against a core-type value.
    TypeIs,
    /// Register the collection on top of the stack as actively iterated.
    IterGuard,
    /// Unregister the innermost active iteration.
    IterGuardEnd,
    /// Propagate an unhandled recoverable `Failure` at a statement boundary.
    CheckFailure,
    /// Push the omitted-argument sentinel for a defaulted parameter the caller skipped.
    Unset,
    /// Jump when a local slot holds a caller-provided value: `u16` slot, `i16` offset.
    ///
    /// The default prologue uses it to skip a default whose parameter the caller supplied,
    /// which is the only way a default can be both lazily and per-call evaluated.
    JumpIfSetLocal,
    /// Publish an instance built by a deferred `BuildStruct`: no operands.
    ///
    /// Restores the declared `fixed` set and evaluates the type's `invariant()` hook, which is
    /// why it is distinct from `BuildStruct`: canon verifies the invariant after `init` runs,
    /// not before.
    SealStruct = 52,
    /// Fail the construction when the top of stack is `false`: `u16` type-name index.
    ///
    /// Emitted after a type's `invariant()` predicate has run. A violated invariant is a
    /// contract fault, not a recoverable `Failure`, because the instance must never be
    /// published in a state canon forbids.
    AssertInvariant = 53,
    /// Assert a signature contract on the top of stack: `u16` type-name index, `u8` nullable
    /// flag, `u16` position-name index.
    ///
    /// Implements `name: Type`, `name!: Type`, `-> T` and `T?` at the call boundary. A
    /// violation is a contract fault, so it is not capturable by `attempt`.
    AssertContract = 54,
    /// Verify the invariant-protected mutations journaled in the current frame: no operands.
    ///
    /// Emitted at stable mutable boundaries. On failure the direct journaled fields return to
    /// their entry values and the operation produces a recoverable `Failure`.
    CheckMutations = 55,
    /// Fill the self-capture upvalue of the closure on top of the stack with itself:
    /// `u16` name index.
    ///
    /// Canon lets a local function reference its own name for recursion. Captures snapshot
    /// values at creation time, so the builder reserves an `Unset` placeholder upvalue and
    /// this instruction — emitted right after the closure is created — rewrites that cell
    /// with the newly created closure, completing the recursion handle.
    FillSelfCapture = 56,
    /// Drive the `Task` on top of the stack to its value: no operands.
    ///
    /// The scheduler runs the awaited task depth-first in deterministic order;
    /// ready tasks resolve immediately, failed ones propagate `Failure`, and
    /// cancelled ones fault (never capturable by `attempt`).
    Await = 57,
    /// Runtime nullable type test of a value against a core-type value (`is ...?`).
    TypeIsNullable = 58,
    /// Read one `each` binding: `u8` mode (`0` primary, `1` key, `2` value).
    ///
    /// Pops the ordinal index and then the collection, pushes the requested projection.
    IterAt = 59,
    /// Specialized load of local slot 0: no operands.
    GetLocal0 = 60,
    /// Specialized load of local slot 1: no operands.
    GetLocal1 = 61,
    /// Specialized load of local slot 2: no operands.
    GetLocal2 = 62,
    /// Specialized load of local slot 3: no operands.
    GetLocal3 = 63,
    /// Specialized load of local slot 4: no operands.
    GetLocal4 = 64,
    /// Specialized load of local slot 5: no operands.
    GetLocal5 = 65,
    /// Specialized load of local slot 6: no operands.
    GetLocal6 = 66,
    /// Specialized load of local slot 7: no operands.
    GetLocal7 = 67,
    /// Specialized call with 0 arguments: no operands.
    Call0 = 68,
    /// Specialized call with 1 argument: no operands.
    Call1 = 69,
    /// Specialized call with 2 arguments: no operands.
    Call2 = 70,
    /// Specialized call with 3 arguments: no operands.
    Call3 = 71,
    /// Specialized call with 4 arguments: no operands.
    Call4 = 72,
    /// Copy the struct on top of the stack, replacing it with the copy: no operands.
    ///
    /// Backs the functional update `base with { ... }`. A struct is a shared
    /// `Rc<RefCell<..>>`, so the update needs its own allocation rather than an alias;
    /// `Dup` would leave both names pointing at one instance. A non-struct on top of the
    /// stack is a `TypeMismatch` fault, matching what the frontend can prove statically.
    CloneStruct = 73,
    /// Test whether the struct on top of the stack matches the named variant: `u16` name index.
    ///
    /// Replaces the value with `Bool(true)` if it is an instance of the variant, `Bool(false)` otherwise.
    IsVariant = 74,
    /// Replace a local slot with a shared cell seeded from its current value: `u16` slot.
    ///
    /// Canon captures a mutable binding *by reference*: a closure that assigns to an
    /// enclosing `var` and that same `var` read in the enclosing frame must observe one
    /// storage location. Emitted immediately before `MakeClosure` for every captured local,
    /// so both sides share the cell rather than the closure snapshotting a copy.
    BoxLocal = 75,
    /// Load a boxed local's inner value: `u16` slot.
    GetBoxedLocal = 76,
    /// Store the top of stack into a boxed local's cell: `u16` slot.
    SetBoxedLocal = 77,
    /// Push a boxed local's cell handle itself, for `MakeClosure`: `u16` slot.
    GetLocalCell = 78,
    /// Push a captured upvalue's cell handle itself, forwarding it to a deeper closure:
    /// `u16` upvalue index.
    GetUpvalueCell = 79,
}

impl TryFrom<u8> for OpCode {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(OpCode::Constant),
            2 => Ok(OpCode::Nil),
            3 => Ok(OpCode::True),
            4 => Ok(OpCode::False),
            5 => Ok(OpCode::Pop),
            6 => Ok(OpCode::Dup),
            7 => Ok(OpCode::GetLocal),
            8 => Ok(OpCode::SetLocal),
            9 => Ok(OpCode::GetGlobal),
            10 => Ok(OpCode::SetGlobal),
            11 => Ok(OpCode::Add),
            12 => Ok(OpCode::Sub),
            13 => Ok(OpCode::Mul),
            14 => Ok(OpCode::Div),
            15 => Ok(OpCode::IntDiv),
            16 => Ok(OpCode::Mod),
            17 => Ok(OpCode::Neg),
            18 => Ok(OpCode::Not),
            19 => Ok(OpCode::Equal),
            20 => Ok(OpCode::NotEqual),
            21 => Ok(OpCode::Less),
            22 => Ok(OpCode::LessEqual),
            23 => Ok(OpCode::Greater),
            24 => Ok(OpCode::GreaterEqual),
            25 => Ok(OpCode::Jump),
            26 => Ok(OpCode::JumpIfFalse),
            27 => Ok(OpCode::Call),
            28 => Ok(OpCode::Return),
            29 => Ok(OpCode::GetField),
            30 => Ok(OpCode::SetField),
            31 => Ok(OpCode::GetIndex),
            32 => Ok(OpCode::SetIndex),
            33 => Ok(OpCode::BuildList),
            34 => Ok(OpCode::BuildDict),
            35 => Ok(OpCode::BuildStruct),
            36 => Ok(OpCode::Fail),
            37 => Ok(OpCode::OrElse),
            38 => Ok(OpCode::PushHandler),
            39 => Ok(OpCode::PopHandler),
            40 => Ok(OpCode::MakeFunction),
            41 => Ok(OpCode::MakeClosure),
            42 => Ok(OpCode::GetUpvalue),
            43 => Ok(OpCode::SetUpvalue),
            44 => Ok(OpCode::Range),
            45 => Ok(OpCode::Len),
            46 => Ok(OpCode::TypeIs),
            47 => Ok(OpCode::IterGuard),
            48 => Ok(OpCode::IterGuardEnd),
            49 => Ok(OpCode::CheckFailure),
            50 => Ok(OpCode::Unset),
            51 => Ok(OpCode::JumpIfSetLocal),
            52 => Ok(OpCode::SealStruct),
            53 => Ok(OpCode::AssertInvariant),
            54 => Ok(OpCode::AssertContract),
            55 => Ok(OpCode::CheckMutations),
            56 => Ok(OpCode::FillSelfCapture),
            57 => Ok(OpCode::Await),
            58 => Ok(OpCode::TypeIsNullable),
            59 => Ok(OpCode::IterAt),
            60 => Ok(OpCode::GetLocal0),
            61 => Ok(OpCode::GetLocal1),
            62 => Ok(OpCode::GetLocal2),
            63 => Ok(OpCode::GetLocal3),
            64 => Ok(OpCode::GetLocal4),
            65 => Ok(OpCode::GetLocal5),
            66 => Ok(OpCode::GetLocal6),
            67 => Ok(OpCode::GetLocal7),
            68 => Ok(OpCode::Call0),
            69 => Ok(OpCode::Call1),
            70 => Ok(OpCode::Call2),
            71 => Ok(OpCode::Call3),
            72 => Ok(OpCode::Call4),
            73 => Ok(OpCode::CloneStruct),
            74 => Ok(OpCode::IsVariant),
            75 => Ok(OpCode::BoxLocal),
            76 => Ok(OpCode::GetBoxedLocal),
            77 => Ok(OpCode::SetBoxedLocal),
            78 => Ok(OpCode::GetLocalCell),
            79 => Ok(OpCode::GetUpvalueCell),
            other => Err(other),
        }
    }
}

/// Constant value stored in the module constant pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    /// Nil value.
    Nil,
    /// Boolean value.
    Bool(bool),
    /// 64-bit integer.
    Int(i64),
    /// 64-bit float.
    Float(f64),
    /// String value.
    String(String),
}
