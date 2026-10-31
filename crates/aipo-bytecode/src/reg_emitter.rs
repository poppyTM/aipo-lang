//! Emitter converting Core IR into 32-bit virtual register instructions (Marco 3 / ADP-014).

use crate::instruction::{RegInstruction, RegOpCode};
use crate::opcode::Constant;
use aipo_ir::{BinaryOp, CoreConstant, CoreFunction, CoreInst, UnaryOp};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// A compiled function for the virtual register machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegCompiledFunction {
    /// Function name.
    pub name: String,
    /// Parameter count (arity).
    pub arity: usize,
    /// `true` for `async fn` (calling produces a `Task` in the stack VM).
    pub is_async: bool,
    /// 32-bit register bytecode instructions.
    pub instructions: Vec<RegInstruction>,
    /// Constant pool.
    pub constants: Vec<Constant>,
    /// Number of virtual registers required.
    pub num_registers: usize,
    /// Signature-contract descriptors referenced by `AssertContract`, in pool order.
    #[serde(default)]
    pub contracts: Vec<RegContract>,
}

/// A signature contract (`name: Type`, `name!: Type`, `-> T`, `T?`) checked at a call
/// boundary by the register engine's `AssertContract` instruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegContract {
    /// Declared type or interface name the value must satisfy.
    pub type_name: String,
    /// `true` for the canonical `T?` form, which also accepts `none`.
    pub nullable: bool,
    /// Where the contract is written (`parameter \`x\``, `return`), for the fault message.
    pub position: String,
    /// Operations an interface contract requires, as `(name, declared arity)`.
    pub operations: Vec<(String, usize)>,
}

/// A compiled module for the virtual register machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegCompiledModule {
    /// Top-level script.
    pub top_level: RegCompiledFunction,
    /// Functions in the module.
    pub functions: Vec<RegCompiledFunction>,
    /// Shared constant pool.
    pub constants: Vec<Constant>,
    /// Declared struct definitions: name -> fields.
    pub struct_defs: HashMap<String, Vec<(String, bool)>>,
}

/// Emitter for converting `CoreFunction` into `RegCompiledFunction`.
#[derive(Debug, Default)]
pub struct RegEmitter {
    constants: Vec<Constant>,
    instructions: Vec<RegInstruction>,
    function_index: HashMap<String, usize>,
    prologue_functions: Vec<(String, usize)>,
    /// Captured enclosing names of the function being compiled, in upvalue index order.
    upvalues: HashMap<String, u16>,
    /// Signature-contract descriptors collected while compiling the current function.
    contracts: Vec<RegContract>,
    max_reg: usize,
}

impl RegEmitter {
    /// Creates a new register emitter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Compiles a Core IR function into a register function.
    ///
    /// # Errors
    /// Rejects unsupported IR and values that cannot fit the fixed instruction encoding.
    #[allow(unused_assignments)]
    pub fn compile_function(mut self, func: &CoreFunction) -> Result<RegCompiledFunction, String> {
        if func.is_async || !func.upvalues.is_empty() {
            return Err(format!(
                "function `{}` requires async or closure support; use --engine=vm",
                func.name
            ));
        }
        let heights = Self::stack_heights(func)?;
        let mut slot_map = HashMap::new();
        for (i, param) in func.params.iter().enumerate() {
            slot_map.insert(param.clone(), i);
        }
        for (i, local) in func.locals.iter().enumerate() {
            slot_map.insert(local.clone(), func.params.len() + i);
        }
        for (i, upvalue) in func.upvalues.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            self.upvalues.insert(upvalue.clone(), i as u16);
        }

        let temp_base = func
            .params
            .len()
            .checked_add(func.locals.len())
            .ok_or("local slot count overflow")?;
        if temp_base >= 256 {
            return Err("function locals leave no temporary register".into());
        }
        let max_height = heights.iter().flatten().copied().max().unwrap_or(0);
        let required = temp_base
            .checked_add(max_height)
            .ok_or("register count overflow")?;
        if required > 256 {
            return Err("evaluation stack exceeds 256 registers".into());
        }
        let top = temp_base;
        self.max_reg = temp_base.max(required.saturating_sub(1));

        // Emit prologue function bindings (without adding to core_to_reg, so jump targets in func remain exact)
        let prologue = std::mem::take(&mut self.prologue_functions);
        for (func_name, func_idx) in prologue {
            let func_reg = top;
            self.track_reg(func_reg);
            self.emit(RegInstruction::encode_abx(
                RegOpCode::MakeFunction,
                func_reg as u8,
                func_idx as u32,
            ));
            let c_idx = self.add_const(Constant::String(func_name));
            self.emit(RegInstruction::encode_abx(
                RegOpCode::SetGlobal,
                func_reg as u8,
                c_idx as u32,
            ));
        }

        let mut core_to_reg: Vec<usize> = Vec::with_capacity(func.instructions.len());
        let mut jump_patches: Vec<(usize, isize, RegOpCode, u8)> = Vec::new();

        for (core_index, inst) in func.instructions.iter().enumerate() {
            // Each branch begins at its incoming CFG height, not the previous
            // textual branch's height. Preflight checks unsupported unreachable IR.
            core_to_reg.push(self.instructions.len());
            let Some(height) = heights[core_index] else {
                continue;
            };
            let mut top = temp_base + height;

            match inst {
                CoreInst::Constant(c, _) => {
                    self.track_reg(top);
                    match c {
                        CoreConstant::None => {
                            self.emit(RegInstruction::encode_abc(
                                RegOpCode::LoadNil,
                                top as u8,
                                0,
                                0,
                            ));
                        }
                        CoreConstant::Bool(b) => {
                            let val = if *b { 1 } else { 0 };
                            self.emit(RegInstruction::encode_abc(
                                RegOpCode::LoadBool,
                                top as u8,
                                val,
                                0,
                            ));
                        }
                        CoreConstant::Int(n) => {
                            if *n >= -65536 && *n <= 65535 {
                                self.emit(RegInstruction::encode_asbx(
                                    RegOpCode::LoadInt,
                                    top as u8,
                                    *n as i32,
                                ));
                            } else {
                                let c_idx = self.add_const(Constant::Int(*n));
                                self.emit(RegInstruction::encode_abx(
                                    RegOpCode::LoadConst,
                                    top as u8,
                                    c_idx as u32,
                                ));
                            }
                        }
                        CoreConstant::Float(f) => {
                            let c_idx = self.add_const(Constant::Float(*f));
                            self.emit(RegInstruction::encode_abx(
                                RegOpCode::LoadConst,
                                top as u8,
                                c_idx as u32,
                            ));
                        }
                        CoreConstant::String(s) => {
                            let c_idx = self.add_const(Constant::String(s.clone()));
                            self.emit(RegInstruction::encode_abx(
                                RegOpCode::LoadConst,
                                top as u8,
                                c_idx as u32,
                            ));
                        }
                    }
                }
                CoreInst::Load(name, _) => {
                    self.track_reg(top);
                    self.emit_load(name, top, &slot_map);
                    top += 1;
                }
                CoreInst::Store(name, _) => {
                    if top > temp_base {
                        top -= 1;
                    }
                    self.emit_store(name, top, &slot_map);
                }
                CoreInst::Binary(op, _) => {
                    if top >= 2 {
                        top -= 2;
                        let left = top;
                        let right = top + 1;
                        let reg_op = match op {
                            BinaryOp::Add => RegOpCode::Add,
                            BinaryOp::Sub => RegOpCode::Sub,
                            BinaryOp::Mul => RegOpCode::Mul,
                            BinaryOp::Div => RegOpCode::Div,
                            BinaryOp::IntDiv => RegOpCode::IntDiv,
                            BinaryOp::Mod => RegOpCode::Mod,
                            BinaryOp::Equal => RegOpCode::Equal,
                            BinaryOp::NotEqual => RegOpCode::NotEqual,
                            BinaryOp::Less => RegOpCode::Less,
                            BinaryOp::LessEqual => RegOpCode::LessEqual,
                            BinaryOp::Greater => RegOpCode::Greater,
                            BinaryOp::GreaterEqual => RegOpCode::GreaterEqual,
                            other => return Err(format!("unsupported binary operator: {other:?}")),
                        };
                        self.emit(RegInstruction::encode_abc(
                            reg_op,
                            top as u8,
                            left as u16,
                            right as u8,
                        ));
                    }
                }
                CoreInst::Unary(op, _) => {
                    if top > 0 {
                        let arg = top - 1;
                        match op {
                            UnaryOp::Neg => {
                                self.emit(RegInstruction::encode_abc(
                                    RegOpCode::Neg,
                                    arg as u8,
                                    arg as u16,
                                    0,
                                ));
                            }
                            UnaryOp::Not => {
                                self.emit(RegInstruction::encode_abc(
                                    RegOpCode::Not,
                                    arg as u8,
                                    arg as u16,
                                    0,
                                ));
                            }
                            UnaryOp::Pos => {}
                        }
                    }
                }
                CoreInst::Jump(target, _) => {
                    let patch_pos = self.instructions.len();
                    self.emit(RegInstruction::encode_asbx(RegOpCode::Jump, 0, 0));
                    jump_patches.push((patch_pos, *target, RegOpCode::Jump, 0));
                }
                CoreInst::JumpIfFalse(target, _) => {
                    if top > temp_base {
                        top -= 1;
                    }
                    let patch_pos = self.instructions.len();
                    self.emit(RegInstruction::encode_asbx(
                        RegOpCode::JumpIfFalse,
                        top as u8,
                        0,
                    ));
                    jump_patches.push((patch_pos, *target, RegOpCode::JumpIfFalse, top as u8));
                }
                CoreInst::JumpIfSetLocal { slot, target, .. } => {
                    if *slot >= temp_base {
                        return Err("default-argument local slot is out of range".into());
                    }
                    let patch_pos = self.instructions.len();
                    self.emit(RegInstruction::encode_asbx(
                        RegOpCode::JumpIfSetLocal,
                        *slot as u8,
                        0,
                    ));
                    jump_patches.push((patch_pos, *target, RegOpCode::JumpIfSetLocal, *slot as u8));
                }
                CoreInst::Return { has_value, .. } => {
                    if *has_value && top > temp_base {
                        top -= 1;
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::Return,
                            top as u8,
                            1,
                            0,
                        ));
                    } else {
                        self.track_reg(top);
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::LoadNil,
                            top as u8,
                            0,
                            0,
                        ));
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::Return,
                            top as u8,
                            1,
                            0,
                        ));
                    }
                }
                CoreInst::Call { arg_count, .. } => {
                    if top > *arg_count {
                        let callee = top - arg_count - 1;
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::Call,
                            callee as u8,
                            *arg_count as u16,
                            1,
                        ));
                    }
                }
                CoreInst::MakeFunction(name, _) => {
                    let func_idx = *self
                        .function_index
                        .get(name)
                        .ok_or_else(|| format!("missing function index for `{name}`"))?;
                    self.track_reg(top);
                    self.emit(RegInstruction::encode_abx(
                        RegOpCode::MakeFunction,
                        top as u8,
                        func_idx as u32,
                    ));
                }
                CoreInst::GetField(name, _) => {
                    if top > 0 {
                        let rec = top - 1;
                        let c_idx = self.add_const(Constant::String(name.clone()));
                        self.emit(RegInstruction::encode_abx(
                            RegOpCode::GetField,
                            rec as u8,
                            c_idx as u32,
                        ));
                    }
                }
                CoreInst::SetField(name, _) => {
                    if top >= 2 {
                        let val = top - 1;
                        let rec = top - 2;
                        let c_idx = self.add_const(Constant::String(name.clone()));
                        if c_idx > 511 {
                            return Err("field name exceeds 9-bit constant operand".into());
                        }
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::SetField,
                            rec as u8,
                            c_idx as u16,
                            val as u8,
                        ));
                    }
                }
                CoreInst::GetIndex(_) => {
                    if top >= 2 {
                        let idx = top - 1;
                        let rec = top - 2;
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::GetIndex,
                            rec as u8,
                            rec as u16,
                            idx as u8,
                        ));
                    }
                }
                CoreInst::SetIndex(_) => {
                    if top >= 3 {
                        let val = top - 1;
                        let idx = top - 2;
                        let rec = top - 3;
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::SetIndex,
                            rec as u8,
                            idx as u16,
                            val as u8,
                        ));
                    }
                }
                CoreInst::Pop(_) => {}
                CoreInst::Dup(_) if top > temp_base => {
                    self.track_reg(top);
                    let prev = top - 1;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::Move,
                        top as u8,
                        prev as u16,
                        0,
                    ));
                }
                CoreInst::BuildList(count, _) if top >= *count => {
                    let start = top - count;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::NewList,
                        start as u8,
                        start as u16,
                        *count as u8,
                    ));
                }
                CoreInst::BuildDict(count, _) if top >= count * 2 => {
                    let reg_count = count * 2;
                    let start = top - reg_count;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::NewDict,
                        start as u8,
                        start as u16,
                        *count as u8,
                    ));
                }
                CoreInst::BuildStruct {
                    type_name,
                    field_count,
                    defer_fixed,
                    ..
                } if top >= *field_count => {
                    let start = top - field_count;
                    let c_idx = self.add_const(Constant::String(type_name.clone()));
                    if c_idx > 511 || *field_count > 255 {
                        return Err("struct operands exceed instruction encoding".into());
                    }
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::NewStruct,
                        start as u8,
                        c_idx as u16,
                        *field_count as u8,
                    ));
                    // The instance is published as "under construction" so `init` may still
                    // assign `fixed` fields. It is sealed here unless the builder defers to an
                    // explicit `SealStruct` after the `init` hook (`defer_fixed`).
                    if !defer_fixed {
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::SealStruct,
                            start as u8,
                            0,
                            0,
                        ));
                    }
                    top = start + 1;
                }
                CoreInst::Range(_) if top >= 2 => {
                    let start = top - 2;
                    let r_start = top - 2;
                    let r_end = top - 1;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::Range,
                        start as u8,
                        r_start as u16,
                        r_end as u8,
                    ));
                }
                CoreInst::CloneStruct(_) if top > 0 => {
                    let rec = top - 1;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::CloneStruct,
                        rec as u8,
                        rec as u16,
                        0,
                    ));
                }
                CoreInst::IsVariant(name, _) if top > 0 => {
                    let rec = top - 1;
                    let c_idx = self.add_const(Constant::String(name.clone()));
                    self.emit(RegInstruction::encode_abx(
                        RegOpCode::IsVariant,
                        rec as u8,
                        c_idx as u32,
                    ));
                }
                CoreInst::Len(_) if top > 0 => {
                    let rec = top - 1;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::Len,
                        rec as u8,
                        rec as u16,
                        0,
                    ));
                }
                CoreInst::PushUnset(_) => {
                    self.track_reg(top);
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::LoadUnset,
                        top as u8,
                        0,
                        0,
                    ));
                }
                CoreInst::IterGuard(_) => {
                    if top > temp_base {
                        top -= 1;
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::IterGuard,
                            top as u8,
                            0,
                            0,
                        ));
                    }
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::IterGuard,
                        top as u8,
                        0,
                        0,
                    ));
                }
                CoreInst::IterGuardEnd(_) => {
                    self.emit(RegInstruction::encode_abc(RegOpCode::IterGuardEnd, 0, 0, 0));
                }
                CoreInst::AssertContract {
                    type_name,
                    nullable,
                    position,
                    operations,
                    ..
                } => {
                    if !operations.is_empty() {
                        return Err("structural interface contracts require --engine=vm".into());
                    }
                    // Keep these constants adjacent; normal deduplication cannot
                    // preserve the Bx+1 position operand contract.
                    let index = self.constants.len();
                    self.constants.push(Constant::String(type_name.clone()));
                    self.constants.push(Constant::String(position.clone()));
                    let opcode = if *nullable {
                        RegOpCode::AssertContractNullable
                    } else {
                        RegOpCode::AssertContract
                    };
                    self.emit(RegInstruction::encode_abx(
                        opcode,
                        (top - 1) as u8,
                        index as u32,
                    ));
                }
                CoreInst::IterAt(mode, _) if top >= 2 => {
                    let coll = top - 2;
                    let idx = top - 1;
                    let opcode = match mode {
                        aipo_ir::IterMode::Primary => RegOpCode::IterPrimary,
                        aipo_ir::IterMode::Key => RegOpCode::IterKey,
                        aipo_ir::IterMode::Value => RegOpCode::IterValue,
                    };
                    self.emit(RegInstruction::encode_abc(
                        opcode,
                        coll as u8,
                        coll as u16,
                        idx as u8,
                    ));
                }
                CoreInst::TypeIs(_) if top >= 2 => {
                    let val = top - 2;
                    let tag = top - 1;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::TypeIs,
                        val as u8,
                        val as u16,
                        tag as u8,
                    ));
                }
                CoreInst::TypeIsNullable(_) if top >= 2 => {
                    let val = top - 2;
                    let tag = top - 1;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::TypeIsNullable,
                        val as u8,
                        val as u16,
                        tag as u8,
                    ));
                }
                CoreInst::PushHandler(target, _) => {
                    self.track_reg(top);
                    let patch_pos = self.instructions.len();
                    self.emit(RegInstruction::encode_asbx(
                        RegOpCode::PushHandler,
                        top as u8,
                        0,
                    ));
                    jump_patches.push((patch_pos, *target, RegOpCode::PushHandler, top as u8));
                }
                CoreInst::PopHandler(_) => {
                    self.emit(RegInstruction::encode_abc(RegOpCode::PopHandler, 0, 0, 0));
                }
                CoreInst::Fail(_) if top > 0 => {
                    let rec = top - 1;
                    self.emit(RegInstruction::encode_abc(RegOpCode::Fail, rec as u8, 0, 0));
                }
                CoreInst::PropagateFailure(_) if top > 0 => {
                    let rec = top - 1;
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::PropagateFailure,
                        rec as u8,
                        0,
                        0,
                    ));
                }
                CoreInst::GetUpvalue(name, _) => {
                    self.track_reg(top);
                    if let Some(&idx) = self.upvalues.get(name) {
                        self.emit(RegInstruction::encode_abx(
                            RegOpCode::GetUpvalue,
                            top as u8,
                            u32::from(idx),
                        ));
                    }
                    top += 1;
                }
                CoreInst::SetUpvalue(name, _) => {
                    if top > temp_base {
                        top -= 1;
                        if let Some(&idx) = self.upvalues.get(name) {
                            self.emit(RegInstruction::encode_abx(
                                RegOpCode::SetUpvalue,
                                top as u8,
                                u32::from(idx),
                            ));
                        }
                    }
                }
                CoreInst::MakeClosure {
                    name,
                    upvalues,
                    self_capture,
                    ..
                } => {
                    // Captures occupy one register each, starting at the closure's own slot, so
                    // `MakeClosure` reads them at `R[A..A+C]` and overwrites `R[A]`.
                    let start = top;
                    for capture in upvalues {
                        self.track_reg(top);
                        if Some(capture.as_str()) == self_capture.as_deref() {
                            // The self capture has no value yet: it *is* the closure being
                            // created, so the sentinel stands in and `FillSelfCapture` rewrites
                            // the cell afterwards.
                            self.emit(RegInstruction::encode_abc(
                                RegOpCode::LoadUnset,
                                top as u8,
                                0,
                                0,
                            ));
                        } else {
                            self.emit_load(capture, top, &slot_map);
                        }
                        top += 1;
                    }
                    let func_idx = *self
                        .function_index
                        .get(name)
                        .expect("function not found in function_index");
                    self.track_reg(start);
                    #[allow(clippy::cast_possible_truncation)]
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::MakeClosure,
                        start as u8,
                        func_idx as u16,
                        upvalues.len() as u8,
                    ));
                    top = start + 1;
                }
                CoreInst::FillSelfCapture { .. } => {
                    if top > temp_base {
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::FillSelfCapture,
                            (top - 1) as u8,
                            0,
                            0,
                        ));
                    }
                }
                CoreInst::Await(_) => {
                    if top > temp_base {
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::Await,
                            (top - 1) as u8,
                            0,
                            0,
                        ));
                    }
                }
                CoreInst::SealStruct(_) => {
                    if top > temp_base {
                        self.emit(RegInstruction::encode_abc(
                            RegOpCode::SealStruct,
                            (top - 1) as u8,
                            0,
                            0,
                        ));
                    }
                }
                CoreInst::AssertInvariant { type_name, .. } => {
                    if top > temp_base {
                        let c_idx = self.add_const(Constant::String(type_name.clone()));
                        self.emit(RegInstruction::encode_abx(
                            RegOpCode::AssertInvariant,
                            (top - 1) as u8,
                            c_idx as u32,
                        ));
                        // `AssertInvariant` consumes the predicate result.
                        top -= 1;
                    }
                }
                CoreInst::CheckMutations(_) => {
                    self.emit(RegInstruction::encode_abc(
                        RegOpCode::CheckMutations,
                        0,
                        0,
                        0,
                    ));
                }
                unsupported => return Err(format!("unsupported register IR: {unsupported:?}")),
            }
        }

        // A jump to the IR end must land on a real implicit return instruction.
        let end_target = self.instructions.len();
        self.emit(RegInstruction::encode_abc(RegOpCode::LoadNil, 0, 0, 0));
        self.emit(RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0));
        for (patch_idx, core_target, op, reg_a) in jump_patches {
            let target = usize::try_from(core_target)
                .ok()
                .filter(|target| *target <= core_to_reg.len())
                .ok_or("jump target outside Core IR")?;
            let target_inst_idx = core_to_reg.get(target).copied().unwrap_or(end_target);
            let offset = (target_inst_idx as i128) - (patch_idx as i128) - 1;
            if !(-65536..=65535).contains(&offset) {
                return Err("jump exceeds signed 17-bit operand".into());
            }
            self.instructions[patch_idx] = RegInstruction::encode_asbx(op, reg_a, offset as i32);
        }
        if self.max_reg >= 256 || self.constants.len() > 131072 {
            return Err("register function exceeds encoding limits".into());
        }

        Ok(RegCompiledFunction {
            name: func.name.clone(),
            arity: func.params.len(),
            is_async: func.is_async,
            instructions: self.instructions,
            constants: self.constants,
            num_registers: (self.max_reg + 1).max(1),
            contracts: self.contracts,
        })
    }

    fn stack_heights(func: &CoreFunction) -> Result<Vec<Option<usize>>, String> {
        let length = func.instructions.len();
        let mut effects = Vec::with_capacity(length);
        for inst in &func.instructions {
            let effect = match inst {
                CoreInst::Constant(..)
                | CoreInst::Load(..)
                | CoreInst::MakeFunction(..)
                | CoreInst::PushUnset(..)
                | CoreInst::GetUpvalue(..)
                | CoreInst::MakeClosure { .. } => (0, 1),
                CoreInst::Store(..)
                | CoreInst::Pop(..)
                | CoreInst::IterGuard(..)
                | CoreInst::JumpIfFalse(..)
                | CoreInst::Fail(..)
                | CoreInst::SetUpvalue(..)
                | CoreInst::AssertInvariant { .. } => (1, 0),
                CoreInst::Binary(op, _) => {
                    if !matches!(
                        op,
                        BinaryOp::Add
                            | BinaryOp::Sub
                            | BinaryOp::Mul
                            | BinaryOp::Div
                            | BinaryOp::IntDiv
                            | BinaryOp::Mod
                            | BinaryOp::Equal
                            | BinaryOp::NotEqual
                            | BinaryOp::Less
                            | BinaryOp::LessEqual
                            | BinaryOp::Greater
                            | BinaryOp::GreaterEqual
                    ) {
                        return Err(format!("unsupported binary operator {op:?}"));
                    }
                    (2, 1)
                }
                CoreInst::GetIndex(..)
                | CoreInst::IterAt(..)
                | CoreInst::Range(..)
                | CoreInst::TypeIs(..)
                | CoreInst::TypeIsNullable(..) => (2, 1),
                CoreInst::SetIndex(..) => (3, 0),
                CoreInst::SetField(..) => (2, 0),
                CoreInst::Unary(..)
                | CoreInst::GetField(..)
                | CoreInst::CloneStruct(..)
                | CoreInst::IsVariant(..)
                | CoreInst::Len(..)
                | CoreInst::PropagateFailure(..)
                | CoreInst::Await(..)
                | CoreInst::SealStruct(..) => (1, 1),
                CoreInst::AssertContract { operations, .. } => {
                    if !operations.is_empty() {
                        return Err("structural interface contracts require --engine=vm".into());
                    }
                    (1, 1)
                }
                CoreInst::Dup(..) => (1, 2),
                // The register engine keeps every local in a flat register, so it has no
                // place for a shared cell. A captured mutable binding needs the canonical
                // engine's cell-based storage; refusing keeps the fallback explicit
                // instead of silently snapshotting the value.
                CoreInst::BoxLocal(..) => {
                    return Err("captured mutable bindings require --engine=vm".into());
                }
                CoreInst::BuildList(count, _) => {
                    if *count > 255 {
                        return Err("list count exceeds 8-bit operand".into());
                    }
                    (*count, 1)
                }
                CoreInst::BuildDict(count, _) => (
                    count.checked_mul(2).ok_or("dictionary operand overflow")?,
                    1,
                ),
                CoreInst::BuildStruct {
                    field_count,
                    defer_fixed,
                    ..
                } => {
                    if *defer_fixed {
                        return Err("construction hooks require --engine=vm".into());
                    }
                    (*field_count, 1)
                }
                CoreInst::Call { arg_count, .. } => {
                    (arg_count.checked_add(1).ok_or("call operand overflow")?, 1)
                }
                CoreInst::Return { has_value, .. } => (usize::from(*has_value), 0),
                CoreInst::Jump(..)
                | CoreInst::JumpIfSetLocal { .. }
                | CoreInst::PushHandler(..)
                | CoreInst::PopHandler(..)
                | CoreInst::IterGuardEnd(..)
                | CoreInst::FillSelfCapture { .. }
                | CoreInst::CheckMutations(..) => (0, 0),
            };
            match inst {
                CoreInst::Jump(target, _)
                | CoreInst::JumpIfFalse(target, _)
                | CoreInst::PushHandler(target, _)
                | CoreInst::JumpIfSetLocal { target, .. }
                    if usize::try_from(*target)
                        .ok()
                        .filter(|target| *target <= length)
                        .is_none() =>
                {
                    return Err("jump target outside Core IR".into());
                }
                _ => {}
            }
            effects.push(effect);
        }
        let mut heights = vec![None; length + 1];
        let mut pending = VecDeque::from([0]);
        heights[0] = Some(0usize);
        while let Some(index) = pending.pop_front() {
            if index == length {
                continue;
            }
            let height = heights[index].ok_or("missing CFG stack height")?;
            let (popped, pushed) = effects[index];
            let next_height = height
                .checked_sub(popped)
                .and_then(|height| height.checked_add(pushed))
                .ok_or_else(|| format!("invalid evaluation stack at IR {index}"))?;
            let mut successors = Vec::with_capacity(2);
            match &func.instructions[index] {
                CoreInst::Return { .. } | CoreInst::Fail(..) => {}
                CoreInst::Jump(target, _) => successors.push((*target as usize, next_height)),
                CoreInst::JumpIfFalse(target, _) | CoreInst::JumpIfSetLocal { target, .. } => {
                    successors.push((*target as usize, next_height));
                    successors.push((index + 1, next_height));
                }
                CoreInst::PushHandler(target, _) => {
                    successors.push((
                        *target as usize,
                        next_height
                            .checked_add(1)
                            .ok_or("handler stack height overflow")?,
                    ));
                    successors.push((index + 1, next_height));
                }
                _ => successors.push((index + 1, next_height)),
            }
            for (successor, incoming) in successors {
                match heights[successor] {
                    Some(existing) if existing != incoming => {
                        return Err(format!(
                            "inconsistent branch stack height at IR {successor}"
                        ));
                    }
                    Some(_) => {}
                    None => {
                        heights[successor] = Some(incoming);
                        pending.push_back(successor);
                    }
                }
            }
        }
        Ok(heights)
    }

    fn emit(&mut self, inst: RegInstruction) {
        self.instructions.push(inst);
    }

    fn track_reg(&mut self, reg: usize) {
        if reg > self.max_reg {
            self.max_reg = reg;
        }
    }

    fn add_const(&mut self, c: Constant) -> usize {
        if let Some(pos) = self.constants.iter().position(|x| x == &c) {
            pos
        } else {
            let pos = self.constants.len();
            self.constants.push(c);
            pos
        }
    }

    /// Emits a load of `name` into register `dest`, resolving locals, captured upvalues and
    /// globals in that order (mirroring the stack emitter's layout).
    fn emit_load(&mut self, name: &str, dest: usize, slots: &HashMap<String, usize>) {
        if let Some(&slot) = slots.get(name) {
            self.emit(RegInstruction::encode_abc(
                RegOpCode::Move,
                dest as u8,
                slot as u16,
                0,
            ));
        } else if let Some(&idx) = self.upvalues.get(name) {
            self.emit(RegInstruction::encode_abx(
                RegOpCode::GetUpvalue,
                dest as u8,
                u32::from(idx),
            ));
        } else {
            let c_idx = self.add_const(Constant::String(name.to_string()));
            self.emit(RegInstruction::encode_abx(
                RegOpCode::GetGlobal,
                dest as u8,
                c_idx as u32,
            ));
        }
    }

    /// Emits a store of register `src` into `name`.
    fn emit_store(&mut self, name: &str, src: usize, slots: &HashMap<String, usize>) {
        if let Some(&slot) = slots.get(name) {
            self.emit(RegInstruction::encode_abc(
                RegOpCode::Move,
                slot as u8,
                src as u16,
                0,
            ));
        } else if let Some(&idx) = self.upvalues.get(name) {
            self.emit(RegInstruction::encode_abx(
                RegOpCode::SetUpvalue,
                src as u8,
                u32::from(idx),
            ));
        } else {
            let c_idx = self.add_const(Constant::String(name.to_string()));
            self.emit(RegInstruction::encode_abx(
                RegOpCode::SetGlobal,
                src as u8,
                c_idx as u32,
            ));
        }
    }

    /// Compiles a CoreModule into a RegCompiledModule.
    ///
    /// # Errors
    /// Returns an error if any function or its operands cannot be represented.
    pub fn compile_module(
        mut self,
        module: &aipo_ir::CoreModule,
    ) -> Result<RegCompiledModule, String> {
        if module.functions.len() > 131072 {
            return Err("function table exceeds 17-bit index".into());
        }
        // First pass: assign function indices so MakeFunction can resolve them.
        for (idx, func) in module.functions.iter().enumerate() {
            self.function_index.insert(func.name.clone(), idx);
        }
        let shared_index = self.function_index.clone();
        let mut functions = Vec::with_capacity(module.functions.len());
        let mut module_constants: Vec<Constant> = Vec::new();

        // Second pass: compile each function with a fresh emitter sharing the index.
        for func in &module.functions {
            let emitter = RegEmitter {
                constants: Vec::new(),
                instructions: Vec::new(),
                function_index: shared_index.clone(),
                prologue_functions: Vec::new(),
                upvalues: HashMap::new(),
                contracts: Vec::new(),
                max_reg: 0,
            };
            let compiled = emitter.compile_function(func)?;
            for c in &compiled.constants {
                if !module_constants.contains(c) {
                    module_constants.push(c.clone());
                }
            }
            functions.push(compiled);
        }

        // Third pass: compile top-level script with function indices available.
        // Binds each declared function as a global before the script body runs.
        let mut prologue_functions = Vec::with_capacity(module.functions.len());
        for (idx, func) in module.functions.iter().enumerate() {
            prologue_functions.push((func.name.clone(), idx));
        }
        let top_emitter = RegEmitter {
            constants: Vec::new(),
            instructions: Vec::new(),
            function_index: shared_index,
            prologue_functions,
            upvalues: HashMap::new(),
            contracts: Vec::new(),
            max_reg: 0,
        };
        let top_level = top_emitter.compile_function(&module.top_level)?;

        let mut struct_defs = HashMap::new();
        for s in &module.structs {
            struct_defs.insert(s.name.clone(), s.fields.clone());
        }

        let compiled = RegCompiledModule {
            top_level,
            functions,
            constants: module_constants,
            struct_defs,
        };
        crate::RegVerifier::verify(&compiled)?;
        Ok(compiled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipo_ir::{BinaryOp, CoreFunction, CoreInst};
    use aipo_source::SourceSpan;

    #[test]
    fn list_count_must_fit_even_when_registers_fit() {
        let span = SourceSpan::default();
        let mut instructions = vec![CoreInst::Constant(CoreConstant::None, span); 256];
        instructions.push(CoreInst::BuildList(256, span));
        let func = CoreFunction {
            name: "too_many_list_items".into(),
            is_async: false,
            params: Vec::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            instructions,
            span,
        };
        assert!(
            RegEmitter::new()
                .compile_function(&func)
                .unwrap_err()
                .contains("8-bit operand")
        );
    }

    #[test]
    fn test_reg_emitter_basic_arithmetic() {
        let dummy_span = SourceSpan::default();
        let func = CoreFunction {
            name: "test".to_string(),
            is_async: false,
            params: vec!["a".to_string(), "b".to_string()],
            locals: vec!["c".to_string()],
            upvalues: Vec::new(),
            instructions: vec![
                CoreInst::Load("a".to_string(), dummy_span),
                CoreInst::Load("b".to_string(), dummy_span),
                CoreInst::Binary(BinaryOp::Add, dummy_span),
                CoreInst::Store("c".to_string(), dummy_span),
                CoreInst::Load("c".to_string(), dummy_span),
                CoreInst::Return {
                    has_value: true,
                    span: dummy_span,
                },
            ],
            span: dummy_span,
        };

        let compiled = RegEmitter::new()
            .compile_function(&func)
            .expect("supported register IR");
        assert_eq!(compiled.name, "test");
        assert!(!compiled.instructions.is_empty());
        assert_eq!(
            compiled.instructions.last().unwrap().opcode(),
            Some(RegOpCode::Return)
        );
    }
}
