//! Emits bytecode from a `CoreModule`.
//!
//! Layout produced by [`BytecodeEmitter::emit`]:
//!
//! ```text
//! ip 0            top-level slot prologue (one `Nil` per declared local)
//!                 `MakeFunction`/`SetGlobal` pair per declared function
//!                 top-level instructions (ending in `Return`)
//! function #0     slot prologue (one `Nil` per non-parameter local), then body
//! function #1     …
//! ```
//!
//! `Load`/`Store` are resolved against the current function layout: parameter and local
//! slots first, then captured upvalues, then globals. Closure creation pushes one
//! enclosing value per capture before `MakeClosure` consumes them.

use crate::module::{AIBC_MAGIC, AIBC_VERSION, BytecodeModule, FunctionInfo, StructInfo};
use crate::opcode::{Constant, OpCode};
use aipo_ir::{BinaryOp, CoreConstant, CoreFunction, CoreInst, CoreModule, IterMode, UnaryOp};
use aipo_source::SourceSpan;
use byteorder::{BigEndian, ByteOrder};
use std::collections::HashMap;

/// Name resolution target inside a function body.
enum Slot {
    /// Parameter or local slot.
    Local(u16),
    /// Captured upvalue index.
    Upvalue(u16),
    /// Global by interned name.
    Global(String),
}

/// Per-function name layout.
struct Layout {
    slots: HashMap<String, u16>,
    upvalues: HashMap<String, u16>,
}

impl Layout {
    fn new(function: &CoreFunction) -> Self {
        let mut slots = HashMap::new();
        for (idx, name) in function.params.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            slots.insert(name.clone(), idx as u16);
        }
        #[allow(clippy::cast_possible_truncation)]
        for (idx, name) in function.locals.iter().enumerate() {
            slots.insert(name.clone(), (function.params.len() + idx) as u16);
        }
        let mut upvalues = HashMap::new();
        for (idx, name) in function.upvalues.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            upvalues.insert(name.clone(), idx as u16);
        }
        Self { slots, upvalues }
    }

    fn resolve(&self, name: &str) -> Slot {
        if let Some(slot) = self.slots.get(name) {
            return Slot::Local(*slot);
        }
        if let Some(idx) = self.upvalues.get(name) {
            return Slot::Upvalue(*idx);
        }
        Slot::Global(name.to_string())
    }
}

/// Compiles a target-neutral Core IR module into executable bytecode.
#[derive(Default)]
pub struct BytecodeEmitter {
    constants: Vec<Constant>,
    constant_ints: HashMap<i64, u16>,
    constant_bools: HashMap<bool, u16>,
    constant_strings: HashMap<String, u16>,
    constant_floats: Vec<(f64, u16)>,
    names: Vec<String>,
    name_map: HashMap<String, u16>,
    code: Vec<u8>,
    spans: Vec<(usize, SourceSpan)>,
    /// (operand offset, target instruction index) within the function being emitted.
    jump_patches: Vec<(usize, usize)>,
    /// Function name to index, used to resolve `MakeFunction`/`MakeClosure`.
    function_index: HashMap<String, usize>,
    /// Absolute byte offset per instruction index of the function being emitted.
    inst_offsets: Vec<usize>,
    /// Operand-width violations collected during emission.
    ///
    /// The bytecode format addresses slots, constants, names and jumps with 16-bit
    /// (and occasionally 8-bit) operands, so a module that exceeds those ranges is
    /// rejected here with a compile-time error instead of silently truncated code.
    errors: Vec<String>,
}

impl BytecodeEmitter {
    /// Creates a new emitter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Emits a `BytecodeModule` from a `CoreModule`.
    ///
    /// # Errors
    /// Returns the collected operand-width violations when the module exceeds the
    /// 16-bit (or 8-bit) operand ranges of the bytecode format.
    pub fn emit(mut self, module: &CoreModule) -> Result<BytecodeModule, Vec<String>> {
        // Runtime function indices are offset by one: entry 0 is always `__top_level__`,
        // so declared function `n` is reachable as index `n + 1`.
        if module.functions.len() >= u16::MAX as usize {
            self.errors.push(format!(
                "function count {} exceeds 16-bit bytecode limit",
                module.functions.len()
            ));
        }
        for (idx, function) in module.functions.iter().enumerate() {
            self.function_index.insert(function.name.clone(), idx + 1);
        }

        let mut functions = Vec::with_capacity(module.functions.len() + 1);
        functions.push(FunctionInfo {
            name: "__top_level__".to_string(),
            entry_ip: 0,
            params: 0,
            locals: module.top_level.locals.len(),
            upvalues: 0,
            is_async: false,
        });

        // Top-level: reserved local slots, function values, then the script body.
        self.check_layout(&module.top_level);
        let top_layout = Layout::new(&module.top_level);
        self.emit_slot_prologue(&module.top_level);
        for (idx, function) in module.functions.iter().enumerate() {
            let fn_idx = self.fit_u16("function index", idx + 1);
            self.code.push(OpCode::MakeFunction as u8);
            self.push_u16(fn_idx);
            let global_idx = self.intern_name(&function.name);
            self.code.push(OpCode::SetGlobal as u8);
            self.push_u16(global_idx);
        }
        self.emit_body(&module.top_level, &top_layout);
        self.patch_jumps();

        for function in &module.functions {
            self.check_layout(function);
            let layout = Layout::new(function);
            let entry = self.code.len();
            functions.push(FunctionInfo {
                name: function.name.clone(),
                entry_ip: entry,
                params: function.params.len(),
                locals: function.locals.len(),
                upvalues: function.upvalues.len(),
                is_async: function.is_async,
            });
            self.emit_slot_prologue(function);
            self.emit_body(function, &layout);
            self.patch_jumps();
        }

        if self.errors.is_empty() {
            Ok(BytecodeModule {
                magic: AIBC_MAGIC,
                version: AIBC_VERSION,
                constants: self.constants,
                names: self.names,
                code: self.code,
                spans: self.spans,
                functions,
                structs: module
                    .structs
                    .iter()
                    .map(|s| StructInfo {
                        name: s.name.clone(),
                        fields: s.fields.clone(),
                    })
                    .collect(),
            })
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    /// Rejects operand counts that do not fit the 16-bit bytecode operand.
    fn fit_u16(&mut self, what: &str, value: usize) -> u16 {
        if value > u16::MAX as usize {
            self.errors.push(format!(
                "{what} count {value} exceeds 16-bit bytecode limit"
            ));
            0
        } else {
            value as u16
        }
    }

    /// Rejects operand counts that do not fit the 8-bit bytecode operand.
    fn fit_u8(&mut self, what: &str, value: usize) -> u8 {
        if value > u8::MAX as usize {
            self.errors
                .push(format!("{what} count {value} exceeds 8-bit bytecode limit"));
            0
        } else {
            value as u8
        }
    }

    /// Rejects per-function layouts whose slot indices do not fit 16 bits.
    fn check_layout(&mut self, function: &CoreFunction) {
        for (what, len) in [
            ("parameter", function.params.len()),
            ("local", function.locals.len()),
            ("upvalue", function.upvalues.len()),
        ] {
            if len > u16::MAX as usize {
                self.errors.push(format!(
                    "{} count {len} in '{}' exceeds 16-bit bytecode limit",
                    what, function.name
                ));
            }
        }
    }

    /// Resolves an absolute jump target, rejecting negative indices.
    fn jump_target(&mut self, raw: isize) -> usize {
        if raw < 0 {
            self.errors.push(format!("jump target {raw} is negative"));
            0
        } else {
            raw as usize
        }
    }

    /// Reserves one `Nil` slot per declared local so `SetLocal` always has a target.
    fn emit_slot_prologue(&mut self, function: &CoreFunction) {
        for _ in 0..function.locals.len() {
            self.code.push(OpCode::Nil as u8);
        }
    }

    fn patch_jumps(&mut self) {
        let end_offset = self.code.len();
        let patches = std::mem::take(&mut self.jump_patches);
        let offsets = std::mem::take(&mut self.inst_offsets);
        for (operand_offset, target_inst) in patches {
            let target_byte = offsets.get(target_inst).copied().unwrap_or(end_offset);
            let next_ip = operand_offset + 2;
            let rel = (target_byte as isize) - (next_ip as isize);
            if rel < i16::MIN as isize || rel > i16::MAX as isize {
                self.errors.push(format!(
                    "jump to instruction {target_inst} exceeds 16-bit relative range"
                ));
            }
            BigEndian::write_i16(
                &mut self.code[operand_offset..operand_offset + 2],
                rel.clamp(i16::MIN as isize, i16::MAX as isize) as i16,
            );
        }
    }

    fn emit_body(&mut self, function: &CoreFunction, layout: &Layout) {
        let mut previous: Option<&CoreInst> = None;
        for inst in &function.instructions {
            let offset = self.code.len();
            self.inst_offsets.push(offset);
            if let Some(previous_inst) = previous
                && matches!(inst, CoreInst::PropagateFailure(_))
                && matches!(
                    previous_inst,
                    CoreInst::Constant(_, _) | CoreInst::MakeFunction(_, _)
                )
            {
                previous = Some(inst);
                continue;
            }
            self.emit_inst(inst, layout);
            previous = Some(inst);
        }
        self.inst_offsets.push(self.code.len());
    }

    fn intern_name(&mut self, name: &str) -> u16 {
        if let Some(&idx) = self.name_map.get(name) {
            return idx;
        }
        let idx = self.fit_u16("interned name", self.names.len());
        self.names.push(name.to_string());
        self.name_map.insert(name.to_string(), idx);
        idx
    }

    fn push_u16(&mut self, value: u16) {
        let mut buf = [0u8; 2];
        BigEndian::write_u16(&mut buf, value);
        self.code.extend_from_slice(&buf);
    }

    fn add_constant(&mut self, constant: Constant) -> u16 {
        let existing = match &constant {
            Constant::Nil => None,
            Constant::Bool(value) => self.constant_bools.get(value).copied(),
            Constant::Int(value) => self.constant_ints.get(value).copied(),
            Constant::Float(value) => self
                .constant_floats
                .iter()
                .find(|(candidate, _)| candidate == value)
                .map(|(_, index)| *index),
            Constant::String(value) => self.constant_strings.get(value).copied(),
        };
        if let Some(index) = existing {
            return index;
        }
        let index = self.fit_u16("constant", self.constants.len());
        match &constant {
            Constant::Bool(value) => {
                self.constant_bools.insert(*value, index);
            }
            Constant::Int(value) => {
                self.constant_ints.insert(*value, index);
            }
            Constant::Float(value) => self.constant_floats.push((*value, index)),
            Constant::String(value) => {
                self.constant_strings.insert(value.clone(), index);
            }
            Constant::Nil => {}
        }
        self.constants.push(constant);
        index
    }

    fn emit_load(&mut self, name: &str, layout: &Layout) {
        match layout.resolve(name) {
            Slot::Local(slot) => match slot {
                0 => self.code.push(OpCode::GetLocal0 as u8),
                1 => self.code.push(OpCode::GetLocal1 as u8),
                2 => self.code.push(OpCode::GetLocal2 as u8),
                3 => self.code.push(OpCode::GetLocal3 as u8),
                4 => self.code.push(OpCode::GetLocal4 as u8),
                5 => self.code.push(OpCode::GetLocal5 as u8),
                6 => self.code.push(OpCode::GetLocal6 as u8),
                7 => self.code.push(OpCode::GetLocal7 as u8),
                _ => {
                    self.code.push(OpCode::GetLocal as u8);
                    self.push_u16(slot);
                }
            },
            Slot::Upvalue(idx) => {
                self.code.push(OpCode::GetUpvalue as u8);
                self.push_u16(idx);
            }
            Slot::Global(name) => {
                let idx = self.intern_name(&name);
                self.code.push(OpCode::GetGlobal as u8);
                self.push_u16(idx);
            }
        }
    }

    fn emit_store(&mut self, name: &str, layout: &Layout) {
        match layout.resolve(name) {
            Slot::Local(slot) => {
                self.code.push(OpCode::SetLocal as u8);
                self.push_u16(slot);
            }
            Slot::Upvalue(idx) => {
                self.code.push(OpCode::SetUpvalue as u8);
                self.push_u16(idx);
            }
            Slot::Global(name) => {
                let idx = self.intern_name(&name);
                self.code.push(OpCode::SetGlobal as u8);
                self.push_u16(idx);
            }
        }
    }

    /// Returns the module function index of a declared or generated function name.
    ///
    /// An unknown name is a compile-time error: falling back to a sentinel would
    /// surface as corrupted bytecode at runtime instead.
    fn function_index_of(&mut self, name: &str) -> u16 {
        match self.function_index.get(name) {
            Some(&idx) => self.fit_u16("function index", idx),
            None => {
                self.errors.push(format!("unknown function '{name}'"));
                0
            }
        }
    }

    fn emit_jump(&mut self, opcode: OpCode, target: usize) {
        self.code.push(opcode as u8);
        let operand = self.code.len();
        self.push_u16(0);
        self.jump_patches.push((operand, target));
    }

    fn emit_inst(&mut self, inst: &CoreInst, layout: &Layout) {
        let offset = self.code.len();
        match inst {
            CoreInst::Constant(c, span) => {
                self.spans.push((offset, *span));
                match c {
                    CoreConstant::None => self.code.push(OpCode::Nil as u8),
                    CoreConstant::Bool(true) => self.code.push(OpCode::True as u8),
                    CoreConstant::Bool(false) => self.code.push(OpCode::False as u8),
                    CoreConstant::Int(n) => {
                        let idx = self.add_constant(Constant::Int(*n));
                        self.code.push(OpCode::Constant as u8);
                        self.push_u16(idx);
                    }
                    CoreConstant::Float(f) => {
                        let idx = self.add_constant(Constant::Float(*f));
                        self.code.push(OpCode::Constant as u8);
                        self.push_u16(idx);
                    }
                    CoreConstant::String(s) => {
                        let idx = self.add_constant(Constant::String(s.clone()));
                        self.code.push(OpCode::Constant as u8);
                        self.push_u16(idx);
                    }
                }
            }
            CoreInst::Load(name, span) => {
                self.spans.push((offset, *span));
                self.emit_load(name, layout);
            }
            CoreInst::Store(name, span) => {
                self.spans.push((offset, *span));
                self.emit_store(name, layout);
            }
            CoreInst::GetUpvalue(name, span) => {
                self.spans.push((offset, *span));
                self.emit_load(name, layout);
            }
            CoreInst::SetUpvalue(name, span) => {
                self.spans.push((offset, *span));
                self.emit_store(name, layout);
            }
            CoreInst::Binary(op, span) => {
                self.spans.push((offset, *span));
                let opcode = match op {
                    BinaryOp::Add => OpCode::Add,
                    BinaryOp::Sub => OpCode::Sub,
                    BinaryOp::Mul => OpCode::Mul,
                    BinaryOp::Div => OpCode::Div,
                    BinaryOp::IntDiv => OpCode::IntDiv,
                    BinaryOp::Mod => OpCode::Mod,
                    BinaryOp::Equal => OpCode::Equal,
                    BinaryOp::NotEqual => OpCode::NotEqual,
                    BinaryOp::Less => OpCode::Less,
                    BinaryOp::LessEqual => OpCode::LessEqual,
                    BinaryOp::Greater => OpCode::Greater,
                    BinaryOp::GreaterEqual => OpCode::GreaterEqual,
                    BinaryOp::OrElse => OpCode::OrElse,
                    other => {
                        self.errors.push(format!(
                            "unlowered binary operator {other:?} at offset {offset}"
                        ));
                        return;
                    }
                };
                self.code.push(opcode as u8);
            }
            CoreInst::Unary(op, span) => {
                self.spans.push((offset, *span));
                let opcode = match op {
                    UnaryOp::Neg => OpCode::Neg,
                    UnaryOp::Not => OpCode::Not,
                    UnaryOp::Pos => return,
                };
                self.code.push(opcode as u8);
            }
            CoreInst::Call { arg_count, span } => {
                self.spans.push((offset, *span));
                match *arg_count {
                    0 => self.code.push(OpCode::Call0 as u8),
                    1 => self.code.push(OpCode::Call1 as u8),
                    2 => self.code.push(OpCode::Call2 as u8),
                    3 => self.code.push(OpCode::Call3 as u8),
                    4 => self.code.push(OpCode::Call4 as u8),
                    _ => {
                        self.code.push(OpCode::Call as u8);
                        let argc = self.fit_u8("call argument", *arg_count);
                        self.code.push(argc);
                    }
                }
            }
            CoreInst::Return { has_value, span } => {
                self.spans.push((offset, *span));
                // `Return` returns the top of the stack, so a valueless return must still
                // push an explicit `none`: the frame's reserved local slots live at the
                // bottom of the stack and must never be mistaken for a result value.
                if !*has_value {
                    self.code.push(OpCode::Nil as u8);
                }
                self.code.push(OpCode::Return as u8);
            }
            CoreInst::Jump(target, span) => {
                self.spans.push((offset, *span));
                let target = self.jump_target(*target);
                self.emit_jump(OpCode::Jump, target);
            }
            CoreInst::JumpIfFalse(target, span) => {
                self.spans.push((offset, *span));
                let target = self.jump_target(*target);
                self.emit_jump(OpCode::JumpIfFalse, target);
            }
            CoreInst::Pop(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::Pop as u8);
            }
            CoreInst::Dup(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::Dup as u8);
            }
            CoreInst::CloneStruct(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::CloneStruct as u8);
            }
            CoreInst::GetField(field, span) => {
                self.spans.push((offset, *span));
                let idx = self.intern_name(field);
                self.code.push(OpCode::GetField as u8);
                self.push_u16(idx);
            }
            CoreInst::SetField(field, span) => {
                self.spans.push((offset, *span));
                let idx = self.intern_name(field);
                self.code.push(OpCode::SetField as u8);
                self.push_u16(idx);
            }
            CoreInst::GetIndex(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::GetIndex as u8);
            }
            CoreInst::SetIndex(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::SetIndex as u8);
            }
            CoreInst::IsVariant(variant, span) => {
                self.spans.push((offset, *span));
                let idx = self.intern_name(variant);
                self.code.push(OpCode::IsVariant as u8);
                self.push_u16(idx);
            }
            CoreInst::BuildList(count, span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::BuildList as u8);
                let count16 = self.fit_u16("list element", *count);
                self.push_u16(count16);
            }
            CoreInst::BuildDict(count, span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::BuildDict as u8);
                let count16 = self.fit_u16("dict entry", *count);
                self.push_u16(count16);
            }
            CoreInst::BuildStruct {
                type_name,
                field_count,
                defer_fixed,
                span,
            } => {
                self.spans.push((offset, *span));
                let type_idx = self.intern_name(type_name);
                self.code.push(OpCode::BuildStruct as u8);
                self.push_u16(type_idx);
                let field_count16 = self.fit_u16("struct field", *field_count);
                self.push_u16(field_count16);
                self.code.push(u8::from(*defer_fixed));
            }
            CoreInst::SealStruct(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::SealStruct as u8);
            }
            CoreInst::AssertInvariant { type_name, span } => {
                self.spans.push((offset, *span));
                let type_idx = self.intern_name(type_name);
                self.code.push(OpCode::AssertInvariant as u8);
                self.push_u16(type_idx);
            }
            CoreInst::AssertContract {
                type_name,
                nullable,
                position,
                operations,
                span,
            } => {
                self.spans.push((offset, *span));
                let type_idx = self.intern_name(type_name);
                let position_idx = self.intern_name(position);
                self.code.push(OpCode::AssertContract as u8);
                self.push_u16(type_idx);
                self.code.push(u8::from(*nullable));
                self.push_u16(position_idx);
                let op_count = self.fit_u8("interface operation", operations.len());
                self.code.push(op_count);
                for (name, arity) in operations {
                    let name_idx = self.intern_name(name);
                    self.push_u16(name_idx);
                    let op_arity = self.fit_u8("interface operation arity", *arity);
                    self.code.push(op_arity);
                }
            }
            CoreInst::CheckMutations(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::CheckMutations as u8);
            }
            CoreInst::MakeFunction(name, span) => {
                self.spans.push((offset, *span));
                let fn_idx = self.function_index_of(name);
                self.code.push(OpCode::MakeFunction as u8);
                self.push_u16(fn_idx);
            }
            CoreInst::MakeClosure {
                name,
                upvalues,
                self_capture,
                span,
            } => {
                self.spans.push((offset, *span));
                for capture in upvalues {
                    // The self capture has no value yet — it *is* the closure being created —
                    // so the sentinel stands in for it and `FillSelfCapture` rewrites the
                    // cell after the closure exists. Loading the name here would read the
                    // creation site's scope, where the local function is not declared.
                    if Some(capture.as_str()) == self_capture.as_deref() {
                        self.code.push(OpCode::Unset as u8);
                        continue;
                    }
                    self.emit_load(capture, layout);
                }
                let fn_idx = self.function_index_of(name);
                self.code.push(OpCode::MakeClosure as u8);
                self.push_u16(fn_idx);
                let upvalues16 = self.fit_u16("closure capture", upvalues.len());
                self.push_u16(upvalues16);
            }
            CoreInst::Range(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::Range as u8);
            }
            CoreInst::Len(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::Len as u8);
            }
            CoreInst::TypeIs(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::TypeIs as u8);
            }
            CoreInst::TypeIsNullable(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::TypeIsNullable as u8);
            }
            CoreInst::IterGuard(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::IterGuard as u8);
            }
            CoreInst::IterAt(mode, span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::IterAt as u8);
                self.code.push(match mode {
                    IterMode::Primary => 0,
                    IterMode::Key => 1,
                    IterMode::Value => 2,
                });
            }
            CoreInst::IterGuardEnd(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::IterGuardEnd as u8);
            }
            CoreInst::Fail(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::Fail as u8);
            }
            CoreInst::PropagateFailure(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::CheckFailure as u8);
            }
            CoreInst::PushUnset(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::Unset as u8);
            }
            CoreInst::FillSelfCapture { name, span } => {
                self.spans.push((offset, *span));
                let idx = self.intern_name(name);
                self.code.push(OpCode::FillSelfCapture as u8);
                self.push_u16(idx);
            }
            CoreInst::JumpIfSetLocal { slot, target, span } => {
                self.spans.push((offset, *span));
                let slot = self.fit_u16("parameter slot", *slot);
                let target = self.jump_target(*target);
                self.code.push(OpCode::JumpIfSetLocal as u8);
                self.push_u16(slot);
                let operand = self.code.len();
                self.push_u16(0);
                self.jump_patches.push((operand, target));
            }
            CoreInst::PushHandler(target, span) => {
                self.spans.push((offset, *span));
                let target = self.jump_target(*target);
                self.emit_jump(OpCode::PushHandler, target);
            }
            CoreInst::PopHandler(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::PopHandler as u8);
            }
            CoreInst::Await(span) => {
                self.spans.push((offset, *span));
                self.code.push(OpCode::Await as u8);
            }
        }
    }
}
