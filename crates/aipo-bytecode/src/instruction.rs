//! 32-bit fixed-width virtual register instruction encoding and decoding (Marco 3 / ADP-014).
//!
//! Inspired by Lua 5.4's register architecture, each instruction occupies exactly
//! 4 bytes (`u32`) aligned, enabling direct decoding without branch mispredictions:
//!
//! ```text
//!  0       6 7             14 15            23 24            31
//! ┌─────────┬────────────────┬────────────────┬────────────────┐
//! │ Opcode  │       A        │       B        │       C        │
//! │ (7 bits)│    (8 bits)    │    (9 bits)    │    (8 bits)    │
//! └─────────┴────────────────┴────────────────┴────────────────┘
//!    iABC:   Opcode   A                B                C
//!    iABx:   Opcode   A                     Bx (17 bits)
//!    iAsBx:  Opcode   A                     sBx (biased 17 bits)
//! ```

use serde::{Deserialize, Serialize};

/// Opcode identifying the virtual register operation (7 bits, 0..=127).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum RegOpCode {
    /// No operation.
    Nop = 0,
    /// Copy register: `R[A] = R[B]`.
    Move = 1,
    /// Load constant from pool: `R[A] = Constants[Bx]`.
    LoadConst = 2,
    /// Load small immediate integer: `R[A] = sBx`.
    LoadInt = 3,
    /// Load `none`: `R[A] = none`.
    LoadNil = 4,
    /// Load boolean: `R[A] = (B != 0)`.
    LoadBool = 5,

    // --- Arithmetic ---
    /// Binary addition: `R[A] = R[B] + R[C]`.
    Add = 10,
    /// Binary subtraction: `R[A] = R[B] - R[C]`.
    Sub = 11,
    /// Binary multiplication: `R[A] = R[B] * R[C]`.
    Mul = 12,
    /// Binary float division: `R[A] = R[B] / R[C]`.
    Div = 13,
    /// Binary integer division: `R[A] = R[B] div R[C]`.
    IntDiv = 14,
    /// Binary modulo: `R[A] = R[B] % R[C]`.
    Mod = 15,
    /// Unary negation: `R[A] = -R[B]`.
    Neg = 16,
    /// Logical NOT: `R[A] = not R[B]`.
    Not = 17,

    // --- Comparisons ---
    /// Equality: `R[A] = (R[B] == R[C])`.
    Equal = 20,
    /// Inequality: `R[A] = (R[B] != R[C])`.
    NotEqual = 21,
    /// Less than: `R[A] = (R[B] < R[C])`.
    Less = 22,
    /// Less or equal: `R[A] = (R[B] <= R[C])`.
    LessEqual = 23,
    /// Greater than: `R[A] = (R[B] > R[C])`.
    Greater = 24,
    /// Greater or equal: `R[A] = (R[B] >= R[C])`.
    GreaterEqual = 25,

    // --- Control Flow ---
    /// Unconditional relative jump: `pc += sBx`.
    Jump = 30,
    /// Jump if true: `if R[A] { pc += sBx }`.
    JumpIfTrue = 31,
    /// Jump if false: `if !R[A] { pc += sBx }`.
    JumpIfFalse = 32,
    /// Jump if local parameter is set (not `Unset`): `if R[A] != Unset { pc += sBx }`.
    JumpIfSetLocal = 33,

    // --- Functions & Calls ---
    /// Call function: `R[A]` with `B` arguments, producing `C` results.
    Call = 40,
    /// Stream pipe call for `|>` pipelines: `R[A]` receives stream from `R[B]` with arg `R[C]`.
    CallPipe = 41,
    /// Return `R[A]`; B and C are reserved.
    Return = 42,
    /// Instantiate / load function: `R[A] = Function[Bx]`.
    MakeFunction = 43,

    // --- Tables, Fields & Structs ---
    /// Read global: `R[A] = Globals[Names[Bx]]`.
    GetGlobal = 50,
    /// Write global: `Globals[Names[Bx]] = R[A]`.
    SetGlobal = 51,
    /// Read named field in place: `R[A] = R[A].field(Names[Bx])`.
    GetField = 52,
    /// Write named field: `R[A].field(Names[B]) = R[C]` (B is a pool index).
    SetField = 53,
    /// Indexed read: `R[A] = R[B][R[C]]`.
    GetIndex = 54,
    /// Indexed write: `R[A][R[B]] = R[C]`.
    SetIndex = 55,
    /// Legacy iteration: index register is `C >> 2`, projection is `C & 3`.
    IterAt = 56,

    // --- Data Structures & Composite Types ---
    /// Build list: `R[A] = [R[B] .. R[B+C-1]]`.
    NewList = 60,
    /// Build dictionary: `R[A] = {R[B]: R[B+1] ..}` (C pairs starting at B).
    NewDict = 61,
    /// Build struct: `R[A] = Struct[B](R[A] .. R[A+C-1])`.
    NewStruct = 62,
    /// Half-open range: `R[A] = R[B]..R[C]`.
    Range = 63,
    /// Test variant: `R[A] = (R[A] is Variant[Bx])`.
    IsVariant = 64,
    /// Length of collection or string: `R[A] = len(R[B])`.
    Len = 65,
    /// Clone struct: `R[A] = clone(R[B])`.
    CloneStruct = 66,
    /// Fail with error value: `R[A] = fail(R[A])`.
    Fail = 67,
    /// Propagate unhandled recoverable failure: returns if `R[A]` is failure.
    PropagateFailure = 68,
    /// Type check: `R[A] = (R[B] is R[C])`.
    TypeIs = 69,
    /// Nullable type check: `R[A] = (R[B] is R[C]?)`.
    TypeIsNullable = 70,
    /// Load `Unset` marker: `R[A] = Unset`.
    LoadUnset = 71,
    /// Push exception/failure handler: pushes handler target `pc += sBx` to handler stack.
    PushHandler = 72,
    /// Pop exception/failure handler from handler stack.
    PopHandler = 73,
    /// Primary iteration projection: full index register C.
    IterPrimary = 74,
    /// Key iteration projection: full index register C.
    IterKey = 75,
    /// Value iteration projection: full index register C.
    IterValue = 76,
    /// Guard structural mutation of collection `R[A]` during iteration.
    IterGuard = 77,
    /// End the current activation's innermost iteration guard.
    IterGuardEnd = 78,
    /// Assert contract on `R[A]`; adjacent pool strings Bx and Bx+1 are type and position.
    AssertContract = 79,
    /// Nullable contract assertion with the same encoding.
    AssertContractNullable = 80,

    // --- Contracts, Invariants & Closures ---
    /// Assert invariant: `assert R[A] is true` for type `Constants[Bx]`.
    AssertInvariant = 81,
    /// Seal struct: seal fixed fields on `R[A]` instance.
    SealStruct = 82,
    /// Create closure: `R[A] = Closure(func_idx: B, upvalues: C starting at R[A])`.
    MakeClosure = 83,
    /// Read captured upvalue: `R[A] = Upvalues[Bx]`.
    GetUpvalue = 84,
    /// Write captured upvalue: `Upvalues[Bx] = R[A]`.
    SetUpvalue = 85,
    /// Fill self-capture cell: fill Unset cell in `R[A]` with `R[A]`.
    FillSelfCapture = 86,
    /// Check mutation boundary: verify journaled mutations.
    CheckMutations = 87,
    /// Await a task: `R[A] = await R[A]`. Suspends the running task when the
    /// operand is a live, non-terminal `Task`.
    Await = 88,
}

impl RegOpCode {
    /// Attempts to convert a 7-bit integer into a `RegOpCode`.
    #[must_use]
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Nop),
            1 => Some(Self::Move),
            2 => Some(Self::LoadConst),
            3 => Some(Self::LoadInt),
            4 => Some(Self::LoadNil),
            5 => Some(Self::LoadBool),
            10 => Some(Self::Add),
            11 => Some(Self::Sub),
            12 => Some(Self::Mul),
            13 => Some(Self::Div),
            14 => Some(Self::IntDiv),
            15 => Some(Self::Mod),
            16 => Some(Self::Neg),
            17 => Some(Self::Not),
            20 => Some(Self::Equal),
            21 => Some(Self::NotEqual),
            22 => Some(Self::Less),
            23 => Some(Self::LessEqual),
            24 => Some(Self::Greater),
            25 => Some(Self::GreaterEqual),
            30 => Some(Self::Jump),
            31 => Some(Self::JumpIfTrue),
            32 => Some(Self::JumpIfFalse),
            33 => Some(Self::JumpIfSetLocal),
            40 => Some(Self::Call),
            41 => Some(Self::CallPipe),
            42 => Some(Self::Return),
            43 => Some(Self::MakeFunction),
            50 => Some(Self::GetGlobal),
            51 => Some(Self::SetGlobal),
            52 => Some(Self::GetField),
            53 => Some(Self::SetField),
            54 => Some(Self::GetIndex),
            55 => Some(Self::SetIndex),
            56 => Some(Self::IterAt),
            60 => Some(Self::NewList),
            61 => Some(Self::NewDict),
            62 => Some(Self::NewStruct),
            63 => Some(Self::Range),
            64 => Some(Self::IsVariant),
            65 => Some(Self::Len),
            66 => Some(Self::CloneStruct),
            67 => Some(Self::Fail),
            68 => Some(Self::PropagateFailure),
            69 => Some(Self::TypeIs),
            70 => Some(Self::TypeIsNullable),
            71 => Some(Self::LoadUnset),
            72 => Some(Self::PushHandler),
            73 => Some(Self::PopHandler),
            74 => Some(Self::IterPrimary),
            75 => Some(Self::IterKey),
            76 => Some(Self::IterValue),
            77 => Some(Self::IterGuard),
            78 => Some(Self::IterGuardEnd),
            79 => Some(Self::AssertContract),
            80 => Some(Self::AssertContractNullable),
            81 => Some(Self::AssertInvariant),
            82 => Some(Self::SealStruct),
            83 => Some(Self::MakeClosure),
            84 => Some(Self::GetUpvalue),
            85 => Some(Self::SetUpvalue),
            86 => Some(Self::FillSelfCapture),
            87 => Some(Self::CheckMutations),
            88 => Some(Self::Await),
            _ => None,
        }
    }
}

/// Bit masks and shift constants for 32-bit instruction encoding.
const OPCODE_MASK: u32 = 0x7F;
const A_MASK: u32 = 0xFF;
const A_SHIFT: u32 = 7;
const B_MASK: u32 = 0x1FF;
const B_SHIFT: u32 = 15;
const C_MASK: u32 = 0xFF;
const C_SHIFT: u32 = 24;

const BX_MASK: u32 = 0x1FFFF;
const BX_SHIFT: u32 = 15;
const SBX_BIAS: i32 = 65536;

/// Encoded 32-bit virtual register instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegInstruction(pub u32);

impl RegInstruction {
    /// Encodes an `iABC` format instruction.
    #[must_use]
    pub fn encode_abc(op: RegOpCode, a: u8, b: u16, c: u8) -> Self {
        let raw = (op as u32 & OPCODE_MASK)
            | ((a as u32 & A_MASK) << A_SHIFT)
            | ((b as u32 & B_MASK) << B_SHIFT)
            | ((c as u32 & C_MASK) << C_SHIFT);
        Self(raw)
    }

    /// Encodes an `iABx` format instruction with unsigned 17-bit immediate.
    #[must_use]
    pub fn encode_abx(op: RegOpCode, a: u8, bx: u32) -> Self {
        let raw = (op as u32 & OPCODE_MASK)
            | ((a as u32 & A_MASK) << A_SHIFT)
            | ((bx & BX_MASK) << BX_SHIFT);
        Self(raw)
    }

    /// Encodes an `iAsBx` format instruction with signed immediate (biased by 65536).
    #[must_use]
    pub fn encode_asbx(op: RegOpCode, a: u8, sbx: i32) -> Self {
        let biased = (sbx + SBX_BIAS) as u32;
        Self::encode_abx(op, a, biased)
    }

    /// Extracts the opcode.
    #[must_use]
    pub fn opcode(self) -> Option<RegOpCode> {
        RegOpCode::from_u8((self.0 & OPCODE_MASK) as u8)
    }

    /// Extracts the raw 7-bit opcode integer.
    #[must_use]
    pub fn raw_opcode(self) -> u8 {
        (self.0 & OPCODE_MASK) as u8
    }

    /// Extracts operand A (8 bits, 0..=255).
    #[must_use]
    pub fn a(self) -> u8 {
        ((self.0 >> A_SHIFT) & A_MASK) as u8
    }

    /// Extracts operand B (9 bits, 0..=511).
    #[must_use]
    pub fn b(self) -> u16 {
        ((self.0 >> B_SHIFT) & B_MASK) as u16
    }

    /// Extracts operand C (8 bits, 0..=255).
    #[must_use]
    pub fn c(self) -> u8 {
        ((self.0 >> C_SHIFT) & C_MASK) as u8
    }

    /// Extracts operand Bx (17 bits unsigned, 0..=131071).
    #[must_use]
    pub fn bx(self) -> u32 {
        (self.0 >> BX_SHIFT) & BX_MASK
    }

    /// Extracts operand sBx (signed, ±65536).
    #[must_use]
    pub fn sbx(self) -> i32 {
        (self.bx() as i32) - SBX_BIAS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iabc_roundtrip() {
        let inst = RegInstruction::encode_abc(RegOpCode::Add, 12, 345, 67);
        assert_eq!(inst.opcode(), Some(RegOpCode::Add));
        assert_eq!(inst.a(), 12);
        assert_eq!(inst.b(), 345);
        assert_eq!(inst.c(), 67);
    }

    #[test]
    fn test_iabx_roundtrip() {
        let inst = RegInstruction::encode_abx(RegOpCode::LoadConst, 42, 123456);
        assert_eq!(inst.opcode(), Some(RegOpCode::LoadConst));
        assert_eq!(inst.a(), 42);
        assert_eq!(inst.bx(), 123456);
    }

    #[test]
    fn test_iasbx_roundtrip_negative_and_positive() {
        let jump_back = RegInstruction::encode_asbx(RegOpCode::Jump, 0, -1024);
        assert_eq!(jump_back.opcode(), Some(RegOpCode::Jump));
        assert_eq!(jump_back.a(), 0);
        assert_eq!(jump_back.sbx(), -1024);

        let jump_forward = RegInstruction::encode_asbx(RegOpCode::JumpIfFalse, 3, 2048);
        assert_eq!(jump_forward.opcode(), Some(RegOpCode::JumpIfFalse));
        assert_eq!(jump_forward.a(), 3);
        assert_eq!(jump_forward.sbx(), 2048);
    }

    #[test]
    fn test_instruction_size_is_exactly_4_bytes() {
        assert_eq!(std::mem::size_of::<RegInstruction>(), 4);
    }
}
