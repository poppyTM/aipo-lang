//! Instruction dispatch for the Aipo stack machine: the `step` loop,
//! operand readers, and every opcode arm. Pure helpers live in `helpers`.
use super::helpers::{collection_identity, length_of, normalize_range, value_diagnostic_name};
use super::{FieldLookup, Vm};
use crate::fault::{VmError, VmFault};
use crate::frame::HandlerFrame;
use crate::value::{
    DictMap, MethodKind, StructInstance, Value, check_finite_float, check_safe_int,
};
use aipo_bytecode::BytecodeModule;
use aipo_bytecode::opcode::{Constant, OpCode};
use byteorder::{BigEndian, ByteOrder};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
impl Vm {
    /// Executes a single instruction at current `ip`.
    ///
    /// Returns `Ok(true)` if execution completed or top-level return executed.
    ///
    /// # Errors
    /// Returns `VmError` on runtime fault or invalid instruction.
    pub fn step(&mut self, module: &BytecodeModule) -> Result<bool, VmError> {
        if self
            .iteration_revisions
            .iter()
            .any(|(value, revision, _)| value.structural_revision() != *revision)
        {
            return Err(VmFault::MutationDuringIteration.into());
        }
        if self.metrics_enabled || self.max_instructions.is_some() {
            self.metrics.instructions = self.metrics.instructions.saturating_add(1);
        }
        if let Some(limit) = self.max_instructions {
            let interval = self.meter_interval.min(limit.max(1));
            if self.metrics.instructions.is_multiple_of(interval)
                && self.metrics.instructions > limit
            {
                return Err(VmFault::Overflow {
                    details: format!(
                        "execution budget exceeded: instruction limit of {limit} reached"
                    ),
                }
                .into());
            }
        }
        if self.host_task.is_some() {
            return self.drive_host_task();
        }
        if self.ip >= module.code.len() || self.halted_with.is_some() {
            if let Some(limit) = self.max_instructions
                && self.metrics.instructions > limit
            {
                return Err(VmFault::Overflow {
                    details: format!(
                        "execution budget exceeded: instruction limit of {limit} reached"
                    ),
                }
                .into());
            }
            return Ok(self.finish_current());
        }

        let opcode_byte = module.code[self.ip];
        self.ip += 1;

        // Dispatch decodes the opcode with a `match` instead of
        // `try_from(..).map_err(..)?` on purpose. The `map_err` form built a 72-byte
        // `VmFault`-carrying `Result` for *every* executed opcode, and its closure captured
        // `self`, so the interpreter paid for a wide error plus a spilled `ip` on the hot path
        // just to format a message that only a corrupt module can trigger. The error here is
        // built on the cold branch only, with the same offset and message as before.
        let opcode = match OpCode::try_from(opcode_byte) {
            Ok(opcode) => opcode,
            Err(byte) => {
                return Err(VmFault::CorruptedBytecode {
                    offset: self.ip - 1,
                    reason: format!("unknown opcode 0x{byte:02x}"),
                }
                .into());
            }
        };

        match opcode {
            OpCode::Constant => {
                let idx = self.read_u16(module)? as usize;
                let val =
                    if let Some(value) = self.constant_values.get(idx) {
                        value.clone()
                    } else {
                        let constant = module.constants.get(idx).ok_or_else(|| {
                            VmFault::CorruptedBytecode {
                                offset: self.ip - 2,
                                reason: format!("constant index {idx} out of bounds"),
                            }
                        })?;
                        match constant {
                            Constant::Nil => Value::None,
                            Constant::Bool(value) => Value::Bool(*value),
                            Constant::Int(value) => Value::Int(check_safe_int(*value)?),
                            Constant::Float(value) => Value::Float(check_finite_float(*value)?),
                            Constant::String(value) => Value::String(Rc::new(value.clone())),
                        }
                    };
                if self.metrics_enabled {
                    self.metrics.constant_loads = self.metrics.constant_loads.saturating_add(1);
                }
                self.push(val)?;
            }
            OpCode::Nil => self.push(Value::None)?,
            OpCode::True => self.push(Value::Bool(true))?,
            OpCode::False => self.push(Value::Bool(false))?,
            OpCode::Unset => self.push(Value::Unset)?,
            OpCode::JumpIfSetLocal => {
                let slot = self.read_u16(module)? as usize;
                let rel = self.read_i16(module)? as isize;
                let base = self.frame_base;
                // A slot holding anything other than the sentinel was supplied by the caller,
                // so its default must not be evaluated; `none` counts as supplied.
                let is_set = !matches!(self.stack.get(base + slot), Some(Value::Unset));
                if is_set {
                    let target = (self.ip as isize) + rel;
                    if target < 0 || (target as usize) > module.code.len() {
                        return Err(VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("jump target {target} out of range"),
                        }
                        .into());
                    }
                    self.ip = target as usize;
                }
            }
            OpCode::Pop => {
                self.pop()?;
            }
            OpCode::Dup => {
                let v = self.peek()?.clone();
                self.push(v)?;
            }
            OpCode::CloneStruct => {
                // Functional update (`base with { ... }`). A `Failure` propagates instead of
                // faulting, so `f()? with { x: 1 }` reaches the handler rather than reporting a
                // type mismatch on a value that was never a struct.
                let value = self.peek()?;
                if value.is_failure() {
                    let failure = self.pop()?;
                    self.handle_failure(failure)?;
                    return Ok(false);
                }

                let instance = match value {
                    Value::Struct(rc) => rc.borrow().clone(),
                    other => {
                        return Err(VmFault::TypeMismatch {
                            expected: "struct".to_string(),
                            actual: other.type_name().to_string(),
                        }
                        .into());
                    }
                };
                let copied = Rc::new(RefCell::new(StructInstance {
                    under_construction: false,
                    ..instance
                }));
                let slot = self
                    .stack
                    .len()
                    .checked_sub(1)
                    .ok_or(VmFault::StackUnderflow)?;
                self.stack[slot] = Value::Struct(copied);
            }
            OpCode::IsVariant => {
                let name_idx = self.read_u16(module)? as usize;
                let expected =
                    module
                        .names
                        .get(name_idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("name index {name_idx} out of bounds"),
                        })?;
                let val = self.pop()?;
                let matches = match &val {
                    Value::Struct(inst) => {
                        let t = &inst.borrow().type_name;
                        t == expected || t.ends_with(&format!(".{expected}"))
                    }
                    _ => false,
                };
                self.push(Value::Bool(matches))?;
            }
            OpCode::GetLocal => {
                let slot = self.read_u16(module)? as usize;
                let base = self.frame_base;
                let val = self
                    .stack
                    .get(base + slot)
                    .cloned()
                    .ok_or(VmFault::StackUnderflow)?;
                if self.metrics_enabled {
                    self.metrics.local_clones = self.metrics.local_clones.saturating_add(1);
                }
                self.push(val)?;
            }
            OpCode::GetLocal0
            | OpCode::GetLocal1
            | OpCode::GetLocal2
            | OpCode::GetLocal3
            | OpCode::GetLocal4
            | OpCode::GetLocal5
            | OpCode::GetLocal6
            | OpCode::GetLocal7 => {
                let slot = (opcode as u8 - OpCode::GetLocal0 as u8) as usize;
                let base = self.frame_base;
                let val = self
                    .stack
                    .get(base + slot)
                    .cloned()
                    .ok_or(VmFault::StackUnderflow)?;
                if self.metrics_enabled {
                    self.metrics.local_clones = self.metrics.local_clones.saturating_add(1);
                }
                self.push(val)?;
            }
            OpCode::SetLocal => {
                let slot = self.read_u16(module)? as usize;
                let base = self.frame_base;
                let val = self.pop()?;
                if base + slot >= self.stack.len() {
                    return Err(VmFault::StackUnderflow.into());
                }
                self.stack[base + slot] = val;
            }
            OpCode::GetGlobal => {
                if self.metrics_enabled {
                    self.metrics.global_lookups = self.metrics.global_lookups.saturating_add(1);
                }
                let idx = self.read_u16(module)? as usize;
                let val = if let Some(value) = self.global_slots.get(idx).and_then(Clone::clone) {
                    value
                } else {
                    let name = module
                        .names
                        .get(idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("name index {idx} out of bounds"),
                        })?;
                    self.globals
                        .get(name)
                        .cloned()
                        .ok_or_else(|| VmFault::UndefinedGlobal { name: name.clone() })?
                };
                if self.metrics_enabled {
                    self.metrics.global_hits = self.metrics.global_hits.saturating_add(1);
                }
                self.push(val)?;
            }
            OpCode::SetGlobal => {
                let idx = self.read_u16(module)? as usize;
                let name = module
                    .names
                    .get(idx)
                    .ok_or_else(|| VmFault::CorruptedBytecode {
                        offset: self.ip - 2,
                        reason: format!("name index {idx} out of bounds"),
                    })?;
                let val = self.pop()?;
                // Publication point: a global outlives the scope that minted any handle in it.
                self.publish_check(&val, || format!("global '{name}'"))?;
                let slot =
                    self.global_slots
                        .get_mut(idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("global slot {idx} out of bounds"),
                        })?;
                *slot = Some(val.clone());
                if let Some(global) = self.globals.get_mut(name) {
                    *global = val;
                } else {
                    self.globals.insert(name.clone(), val);
                }
            }
            OpCode::Add => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.add(&b)?;
                self.push(res)?;
            }
            OpCode::Sub => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.sub(&b)?;
                self.push(res)?;
            }
            OpCode::Mul => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.mul(&b)?;
                self.push(res)?;
            }
            OpCode::Div => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.div(&b)?;
                self.push(res)?;
            }
            OpCode::IntDiv => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.int_div(&b)?;
                self.push(res)?;
            }
            OpCode::Mod => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.modulo(&b)?;
                self.push(res)?;
            }
            OpCode::Neg => {
                let a = self.pop()?;
                let res = a.negate()?;
                self.push(res)?;
            }
            OpCode::Not => {
                let a = self.pop()?;
                let res = a.not()?;
                self.push(res)?;
            }
            OpCode::Equal => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.equal(&b)?;
                self.push(res)?;
            }
            OpCode::NotEqual => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.not_equal(&b)?;
                self.push(res)?;
            }
            OpCode::Less => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.less(&b)?;
                self.push(res)?;
            }
            OpCode::LessEqual => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.less_equal(&b)?;
                self.push(res)?;
            }
            OpCode::Greater => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.greater(&b)?;
                self.push(res)?;
            }
            OpCode::GreaterEqual => {
                let b = self.pop()?;
                let a = self.pop()?;
                let res = a.greater_equal(&b)?;
                self.push(res)?;
            }
            OpCode::Jump => {
                let rel = self.read_i16(module)? as isize;
                let target = (self.ip as isize) + rel;
                if target < 0 || (target as usize) > module.code.len() {
                    return Err(VmFault::CorruptedBytecode {
                        offset: self.ip - 2,
                        reason: format!("jump target {target} out of range"),
                    }
                    .into());
                }
                self.ip = target as usize;
            }
            OpCode::JumpIfFalse => {
                let rel = self.read_i16(module)? as isize;
                let cond = self.pop()?;
                let is_true = cond.as_bool()?;
                if !is_true {
                    let target = (self.ip as isize) + rel;
                    if target < 0 || (target as usize) > module.code.len() {
                        return Err(VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("jump target {target} out of range"),
                        }
                        .into());
                    }
                    self.ip = target as usize;
                }
            }
            OpCode::Call => {
                self.call_inst_len = 2;
                let arg_count = self.read_u8(module)? as usize;
                self.begin_call(module, arg_count)?;
            }
            OpCode::Call0 | OpCode::Call1 | OpCode::Call2 | OpCode::Call3 | OpCode::Call4 => {
                self.call_inst_len = 1;
                let arg_count = (opcode as u8 - OpCode::Call0 as u8) as usize;
                self.begin_call(module, arg_count)?;
            }
            OpCode::Await => {
                // No operands: rewinding resumes by re-executing this opcode.
                let await_ip = self.ip - 1;
                self.op_await(await_ip)?;
            }
            OpCode::Return => {
                let ret_val = if self.stack.is_empty() {
                    Value::None
                } else {
                    self.pop()?
                };

                // Publication point: a returned value becomes reachable in the caller's frame,
                // which is outside the scope a host callback ran in.
                self.publish_check(&ret_val, || "a return value".to_string())?;
                if let Some(frame) = self.frames.pop() {
                    self.unwind_iterations(self.frames.len());
                    self.refresh_frame_base();
                    self.upvalue_frames.pop();
                    self.mutation_journal.truncate(frame.journal_start);
                    if self.frames.is_empty()
                        && self.current.is_some()
                        && self.current != Some(super::task::MAIN_TASK)
                    {
                        self.stack.clear();
                        return Ok(
                            self.complete_current(super::task::TaskOutcome::from_value(ret_val))
                        );
                    }
                    self.stack.truncate(frame.result_base());
                    self.push(ret_val)?;
                    self.ip = frame.return_ip;
                } else {
                    // Top-level return completes the loaded task (or the entry
                    // script): waiters and joins resolve from there.
                    return Ok(self.complete_current(super::task::TaskOutcome::from_value(ret_val)));
                }
            }
            OpCode::GetField => {
                if self.metrics_enabled {
                    self.metrics.field_lookups = self.metrics.field_lookups.saturating_add(1);
                }
                let name_idx = self.read_u16(module)? as usize;
                let field_name =
                    module
                        .names
                        .get(name_idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("name index {name_idx} out of bounds"),
                        })?;

                let target = self.pop()?;
                if let Value::Failure(err) = &target {
                    if field_name == "message" {
                        self.push(Value::String(Rc::new(err.message.clone())))?;
                        return Ok(false);
                    }
                    if field_name == "payload" {
                        self.push(err.payload.clone())?;
                        return Ok(false);
                    }
                    self.push(target)?;
                    return Ok(false);
                }

                match &target {
                    Value::Struct(inst) => {
                        let field = {
                            let instance = inst.borrow();
                            match self.lookup_struct_field(
                                name_idx,
                                &instance.type_name,
                                field_name,
                            ) {
                                FieldLookup::Field(index) => {
                                    instance.fields.get(index).map(|(_, value)| value.clone())
                                }
                                FieldLookup::Missing => instance.get_field(field_name).cloned(),
                            }
                        };
                        if let Some(val) = field {
                            if self.metrics_enabled {
                                self.metrics.field_hits = self.metrics.field_hits.saturating_add(1);
                            }
                            self.push(val)?;
                        } else {
                            let type_name = inst.borrow().type_name.clone();
                            if self.metrics_enabled {
                                self.metrics.field_hits = self.metrics.field_hits.saturating_add(1);
                            }
                            if let Some((entry_ip, total_arity, is_async)) =
                                self.lookup_struct_method(&type_name, field_name)
                            {
                                self.push(Value::struct_method(
                                    inst.clone(),
                                    entry_ip as u32,
                                    total_arity as u16,
                                    is_async,
                                ))?;
                            } else if let Some((arity, func)) = self
                                .method_natives
                                .get(&(type_name.clone(), field_name.clone()))
                            {
                                self.push(Value::bound_method(
                                    format!("{type_name}.{field_name}"),
                                    *arity,
                                    target.clone(),
                                    MethodKind::Native(*func),
                                ))?;
                            } else {
                                return Err(VmFault::NoSuchField {
                                    type_name,
                                    field: field_name.clone(),
                                }
                                .into());
                            }
                        }
                    }
                    Value::Dict(d) => {
                        let key = Value::String(Rc::new(field_name.clone()));
                        let found = d.borrow().get(&key).cloned();
                        if let Some(v) = found {
                            if self.metrics_enabled {
                                self.metrics.field_hits = self.metrics.field_hits.saturating_add(1);
                            }
                            self.push(v)?;
                        } else if let Some(method) = self.bind_method(&target, field_name) {
                            if self.metrics_enabled {
                                self.metrics.field_hits = self.metrics.field_hits.saturating_add(1);
                            }
                            self.push(method)?;
                        } else {
                            return Err(VmFault::KeyNotFound {
                                key: field_name.clone(),
                            }
                            .into());
                        }
                    }
                    other => {
                        if let Some(method) = self.bind_method(other, field_name) {
                            if self.metrics_enabled {
                                self.metrics.field_hits = self.metrics.field_hits.saturating_add(1);
                            }
                            self.push(method)?;
                        } else {
                            return Err(VmFault::TypeMismatch {
                                expected: "struct, collection, or type with that member"
                                    .to_string(),
                                actual: format!("{}.{field_name}", other.type_name()),
                            }
                            .into());
                        }
                    }
                }
            }
            OpCode::SetField => {
                let name_idx = self.read_u16(module)? as usize;
                let field_name =
                    module
                        .names
                        .get(name_idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("name index {name_idx} out of bounds"),
                        })?;

                let new_val = self.pop()?;
                let target = self.pop()?;

                // A `Failure` on either side propagates instead of faulting, exactly like
                // the indexed form (auditoria IR-11). Propagation is immediate because an
                // assignment is a statement, and the model's statement boundary is where
                // an unconsumed `Failure` ends the path; leaving it on the operand stack
                // would expose the frame's reserved local slots.
                if target.is_failure() {
                    self.handle_failure(target)?;
                    return Ok(false);
                }
                if new_val.is_failure() {
                    self.handle_failure(new_val)?;
                    return Ok(false);
                }

                // Publication point: the field outlives the assignment's scope.
                self.publish_check(&new_val, || format!("field '{field_name}'"))?;

                // Self-reference cycle guard: storing a struct into its own field would
                // create a cycle the reference counter cannot collect.
                if target.is_same_allocation(&new_val) {
                    let failure = Value::failure_with_payload(
                        "cannot store a struct into its own field: self-referential cycle",
                        Value::String(std::rc::Rc::new("cycle".to_string())),
                    );
                    self.handle_failure(failure)?;
                    return Ok(false);
                }

                if let Value::Struct(inst) = &target {
                    // Canon applies a guarded update provisionally and verifies it at the end
                    // of the enclosing mutable operation, so the frame-entry value is kept for
                    // a possible rollback instead of validating the assignment right here.
                    let (entry_value, published, guarded, field_slot) = {
                        let instance = inst.borrow();
                        let field_slot =
                            self.lookup_struct_field(name_idx, &instance.type_name, field_name);
                        let guarded = self.type_is_guarded(&instance.type_name);
                        let entry_value = if guarded {
                            match field_slot {
                                FieldLookup::Field(index) => {
                                    instance.fields.get(index).map(|(_, value)| value.clone())
                                }
                                FieldLookup::Missing => instance.get_field(field_name).cloned(),
                            }
                        } else {
                            None
                        };
                        (
                            entry_value,
                            !instance.under_construction,
                            guarded,
                            field_slot,
                        )
                    };
                    let mut instance = inst.borrow_mut();
                    if let FieldLookup::Field(index) = field_slot {
                        if instance.fields.get(index).is_some() {
                            instance.set_field_at(index, field_name, new_val)?;
                        } else {
                            instance.set_field(field_name, new_val)?;
                        }
                    } else {
                        instance.set_field(field_name, new_val)?;
                    }
                    drop(instance);

                    if published
                        && guarded
                        && let Some(previous) = entry_value
                    {
                        self.journal_mutation(Rc::clone(inst), field_name.clone(), previous);
                    }
                } else {
                    return Err(VmFault::TypeMismatch {
                        expected: "struct instance".to_string(),
                        actual: target.type_name().to_string(),
                    }
                    .into());
                }
            }
            OpCode::GetIndex => {
                let index = self.pop()?;
                let target = self.pop()?;

                if target.is_failure() {
                    self.push(target)?;
                    return Ok(false);
                }
                if index.is_failure() {
                    self.push(index)?;
                    return Ok(false);
                }

                match (&target, &index) {
                    (Value::List(l), Value::Range(r)) => {
                        let items = l.borrow().clone();
                        let (from, to) = normalize_range(r.start, r.end, items.len());
                        let slice: Vec<Value> = items[from..to].to_vec();
                        self.push(Value::List(Rc::new(RefCell::new((slice).into()))))?;
                    }
                    (Value::String(s), Value::Range(r)) => {
                        let chars: Vec<char> = s.chars().collect();
                        let (from, to) = normalize_range(r.start, r.end, chars.len());
                        let slice: String = chars[from..to].iter().collect();
                        self.push(Value::String(Rc::new(slice)))?;
                    }
                    (Value::Bytes(b), Value::Range(r)) => {
                        let bytes = b.borrow();
                        let (from, to) = normalize_range(r.start, r.end, bytes.len());
                        self.push(Value::Bytes(Rc::new(RefCell::new(
                            (bytes[from..to].to_vec()).into(),
                        ))))?;
                    }
                    (Value::Range(r), Value::Int(i)) => {
                        // Positional access into a half-open range, which is what makes
                        // `each i in 0..n` (documented canon) lower to the same
                        // index-based loop every other iterable uses.
                        let len = usize::try_from((r.end - r.start).max(0)).unwrap_or(0);
                        #[allow(clippy::cast_possible_wrap)]
                        let actual = if *i < 0 { (len as i64) + *i } else { *i };
                        let resolved = usize::try_from(actual)
                            .ok()
                            .filter(|index| *index < len)
                            .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                        #[allow(clippy::cast_possible_wrap)]
                        let offset = resolved as i64;
                        self.push(Value::Int(r.start + offset))?;
                    }
                    (Value::Bytes(b), Value::Int(i)) => {
                        let bytes = b.borrow();
                        let len = bytes.len();
                        #[allow(clippy::cast_possible_wrap)]
                        let actual_idx = if *i < 0 { (len as i64) + *i } else { *i };
                        let idx_usize = usize::try_from(actual_idx)
                            .map_err(|_| VmFault::IndexOutOfRange { index: *i, len })?;
                        let byte = *bytes
                            .get(idx_usize)
                            .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                        self.push(Value::Byte(byte))?;
                    }
                    (Value::List(l), Value::Int(i)) => {
                        let len = l.borrow().len();
                        #[allow(clippy::cast_possible_wrap)]
                        let actual_idx = if *i < 0 { (len as i64) + *i } else { *i };
                        let idx_usize = usize::try_from(actual_idx)
                            .map_err(|_| VmFault::IndexOutOfRange { index: *i, len })?;
                        if idx_usize >= len {
                            return Err(VmFault::IndexOutOfRange { index: *i, len }.into());
                        }
                        let val = l.borrow()[idx_usize].clone();
                        self.push(val)?;
                    }
                    (Value::Sequence(pipeline), Value::Int(i)) => {
                        // Indexing drains (memoized), so `each` over a
                        // sequence evaluates element callbacks exactly once.
                        let items = self.sequence_items(module, pipeline)?;
                        let len = items.len();
                        #[allow(clippy::cast_possible_wrap)]
                        let actual_idx = if *i < 0 { (len as i64) + *i } else { *i };
                        let idx_usize = usize::try_from(actual_idx)
                            .map_err(|_| VmFault::IndexOutOfRange { index: *i, len })?;
                        let val = items
                            .get(idx_usize)
                            .cloned()
                            .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                        self.push(val)?;
                    }
                    (Value::Dict(d), _) => {
                        if let Some(v) = d.borrow().get(&index) {
                            self.push(v.clone())?;
                        } else {
                            return Err(VmFault::KeyNotFound {
                                key: format!("{index}"),
                            }
                            .into());
                        }
                    }
                    (Value::String(s), Value::Int(i)) => {
                        self.push(Value::String(Rc::new(crate::value::string_char_at(s, *i)?)))?;
                    }
                    _ => {
                        return Err(VmFault::TypeMismatch {
                            expected: "indexable collection or string".to_string(),
                            actual: format!(
                                "{} indexed by {}",
                                target.type_name(),
                                index.type_name()
                            ),
                        }
                        .into());
                    }
                }
            }
            OpCode::SetIndex => {
                let new_val = self.pop()?;
                let index = self.pop()?;
                let target = self.pop()?;

                if target.is_failure() {
                    self.handle_failure(target)?;
                    return Ok(false);
                }
                if index.is_failure() {
                    self.handle_failure(index)?;
                    return Ok(false);
                }
                if new_val.is_failure() {
                    self.handle_failure(new_val)?;
                    return Ok(false);
                }
                // Publication point: the stored element outlives the assignment's scope.
                self.publish_check(&new_val, || "an indexed element".to_string())?;

                // Self-reference cycle guard: storing a container into itself would create
                // a cycle the reference counter cannot collect.
                if target.is_same_allocation(&new_val) {
                    let failure = Value::failure_with_payload(
                        "cannot store a collection into itself: self-referential cycle",
                        Value::String(std::rc::Rc::new("cycle".to_string())),
                    );
                    self.handle_failure(failure)?;
                    return Ok(false);
                }

                match (&target, &index) {
                    (Value::List(l), Value::Int(i)) => {
                        let len = l.borrow().len();
                        #[allow(clippy::cast_possible_wrap)]
                        let actual_idx = if *i < 0 { (len as i64) + *i } else { *i };
                        let idx_usize = usize::try_from(actual_idx)
                            .map_err(|_| VmFault::IndexOutOfRange { index: *i, len })?;
                        if idx_usize >= len {
                            return Err(VmFault::IndexOutOfRange { index: *i, len }.into());
                        }
                        l.borrow_mut()[idx_usize] = new_val;
                    }
                    (Value::Dict(d), _) => {
                        d.borrow_mut().upsert(index, new_val);
                    }
                    (Value::Bytes(b), Value::Int(i)) => {
                        let len = b.borrow().len();
                        #[allow(clippy::cast_possible_wrap)]
                        let actual_idx = if *i < 0 { (len as i64) + *i } else { *i };
                        let idx_usize = usize::try_from(actual_idx)
                            .map_err(|_| VmFault::IndexOutOfRange { index: *i, len })?;
                        if idx_usize >= len {
                            return Err(VmFault::IndexOutOfRange { index: *i, len }.into());
                        }
                        let byte_val = match &new_val {
                            Value::Byte(byte) => *byte,
                            Value::Int(n) if (0..=255).contains(n) => *n as u8,
                            other => {
                                return Err(VmFault::TypeMismatch {
                                    expected: "Byte or Int in 0..=255".to_string(),
                                    actual: other.type_name().to_string(),
                                }
                                .into());
                            }
                        };
                        b.borrow_mut()[idx_usize] = byte_val;
                    }
                    _ => {
                        return Err(VmFault::TypeMismatch {
                            expected: "mutable collection".to_string(),
                            actual: target.type_name().to_string(),
                        }
                        .into());
                    }
                }
            }
            OpCode::BuildList => {
                let count = self.read_u16(module)? as usize;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(self.pop()?);
                }
                items.reverse();
                // Publication point: a handle smuggled into a list literal has already left its
                // scope, even if the list itself is never stored anywhere.
                for item in &items {
                    self.publish_check(item, || "a list element".to_string())?;
                }
                self.push(Value::List(Rc::new(RefCell::new((items).into()))))?;
            }
            OpCode::BuildDict => {
                let count = self.read_u16(module)? as usize;
                let mut pairs = Vec::with_capacity(count);
                for _ in 0..count {
                    let v = self.pop()?;
                    let k = self.pop()?;
                    pairs.push((k, v));
                }
                pairs.reverse();
                // Publication point: same rule as a list literal, for both halves of the entry.
                for (key, value) in &pairs {
                    self.publish_check(key, || "a dict key".to_string())?;
                    self.publish_check(value, || "a dict entry".to_string())?;
                }
                self.push(Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(
                    pairs,
                )))))?;
            }
            OpCode::BuildStruct => {
                let type_idx = self.read_u16(module)? as usize;
                let field_count = self.read_u16(module)? as usize;
                let defer_fixed = self.read_u8(module)? != 0;
                let type_name =
                    module
                        .names
                        .get(type_idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 5,
                            reason: format!("type name index {type_idx} out of bounds"),
                        })?;

                let mut values = Vec::with_capacity(field_count);
                for _ in 0..field_count {
                    values.push(self.pop()?);
                }
                values.reverse();

                let instance = if let Some(defs) = self.struct_defs.get(type_name) {
                    let mut fields = Vec::new();
                    let mut fixed_fields = HashSet::new();
                    for ((f_name, is_fixed), val) in defs.iter().cloned().zip(values) {
                        // A deferred construction keeps `fixed` fields mutable until `init`
                        // finishes and the instance is sealed.
                        if is_fixed && !defer_fixed {
                            fixed_fields.insert(f_name.clone());
                        }
                        fields.push((f_name, val));
                    }
                    StructInstance {
                        type_name: type_name.clone(),
                        fields,
                        fixed_fields,
                        under_construction: defer_fixed,
                    }
                } else {
                    let fields = values
                        .into_iter()
                        .enumerate()
                        .map(|(i, v)| (format!("field_{i}"), v))
                        .collect();
                    StructInstance {
                        type_name: type_name.clone(),
                        fields,
                        fixed_fields: HashSet::new(),
                        under_construction: false,
                    }
                };

                // While `init` still has to run, the invariant is evaluated at seal time
                // instead, so it observes the fields `init` assigned.
                if !defer_fixed && let Some(validator) = self.struct_invariants.get(type_name) {
                    validator(&instance).map_err(|msg| VmFault::InvariantViolation {
                        type_name: type_name.clone(),
                        message: msg,
                    })?;
                }

                self.push(Value::Struct(Rc::new(RefCell::new(instance))))?;
            }
            OpCode::SealStruct => {
                let value = self.pop()?;
                let instance = match &value {
                    Value::Struct(instance) => Rc::clone(instance),
                    other => {
                        let actual = other.type_name().to_string();
                        self.push(value)?;
                        return Err(VmFault::TypeMismatch {
                            expected: "Struct under construction for SealStruct".to_string(),
                            actual,
                        }
                        .into());
                    }
                };
                let type_name = instance.borrow().type_name.clone();
                let fixed_fields: HashSet<String> = self
                    .struct_defs
                    .get(&type_name)
                    .map(|defs| {
                        defs.iter()
                            .filter(|(_, is_fixed)| *is_fixed)
                            .map(|(name, _)| name.clone())
                            .collect()
                    })
                    .unwrap_or_default();

                {
                    let mut borrowed = instance.borrow_mut();
                    borrowed.seal(fixed_fields);
                    if let Some(validator) = self.struct_invariants.get(&type_name) {
                        validator(&borrowed).map_err(|msg| VmFault::InvariantViolation {
                            type_name: type_name.clone(),
                            message: msg,
                        })?;
                    }
                }

                self.push(Value::Struct(instance))?;
            }
            OpCode::AssertInvariant => {
                let name_idx = self.read_u16(module)? as usize;
                let type_name = module.names.get(name_idx).cloned().ok_or_else(|| {
                    VmFault::CorruptedBytecode {
                        offset: self.ip - 2,
                        reason: format!("type name index {name_idx} out of bounds"),
                    }
                })?;
                let result = self.pop()?;
                if !matches!(result, Value::Bool(true)) {
                    return Err(VmFault::InvariantViolation {
                        type_name,
                        message: "invariant() did not hold after construction".to_string(),
                    }
                    .into());
                }
            }
            OpCode::AssertContract => {
                let type_idx = self.read_u16(module)? as usize;
                let nullable = self.read_u8(module)? != 0;
                let position_idx = self.read_u16(module)? as usize;
                let type_name = module.names.get(type_idx).cloned().ok_or_else(|| {
                    VmFault::CorruptedBytecode {
                        offset: self.ip - 5,
                        reason: format!("contract type index {type_idx} out of bounds"),
                    }
                })?;
                let position = module.names.get(position_idx).cloned().ok_or_else(|| {
                    VmFault::CorruptedBytecode {
                        offset: self.ip - 5,
                        reason: format!("contract position index {position_idx} out of bounds"),
                    }
                })?;
                let operation_count = self.read_u8(module)? as usize;
                let mut operations = Vec::with_capacity(operation_count);
                for _ in 0..operation_count {
                    let name_idx = self.read_u16(module)? as usize;
                    let arity = self.read_u8(module)? as usize;
                    let name = module.names.get(name_idx).cloned().ok_or_else(|| {
                        VmFault::CorruptedBytecode {
                            offset: self.ip - 3,
                            reason: format!("contract operation index {name_idx} out of bounds"),
                        }
                    })?;
                    operations.push((name, arity));
                }
                // The contract inspects the value in place so the same instruction serves a
                // parameter check (after `Load`) and a return check (before `Return`).
                let value = self.stack.last().cloned().ok_or(VmFault::StackUnderflow)?;
                // An omitted argument keeps the sentinel (reported as a missing argument, not a
                // type violation) and `fail(...)` ends the path without satisfying the return
                // contract, so neither is a contract violation.
                let skippable = matches!(value, Value::Unset | Value::Failure(_));
                if !skippable && !self.contract_holds(&type_name, nullable, &value) {
                    return Err(VmFault::ContractViolation {
                        position,
                        expected: if nullable {
                            format!("{type_name}?")
                        } else {
                            type_name
                        },
                        actual: value_diagnostic_name(&value),
                    }
                    .into());
                }
                // Canon makes an interface structural, so the contract asks whether the value
                // exposes the operations the interface declares. `none` satisfies `T?` without
                // exposing anything, which is exactly what `T?` means.
                if !skippable && !matches!(value, Value::None) {
                    for (operation, declared) in &operations {
                        let actual = self.operation_arity(&value, operation);
                        if actual != Some(*declared) {
                            // The contract names the *caller-visible* arity (the receiver is not
                            // an argument at the call site), which is also how native method
                            // arities are declared.
                            let required = format!("{type_name}.{operation}/{declared}");
                            let actual_name = value_diagnostic_name(&value);
                            let found = match actual {
                                Some(arity) => format!("{actual_name}.{operation}/{arity}"),
                                None => format!("{actual_name} without '{operation}'"),
                            };
                            return Err(VmFault::ContractViolation {
                                position,
                                expected: required,
                                actual: found,
                            }
                            .into());
                        }
                    }
                }
            }
            OpCode::CheckMutations => {
                let base = self.frames.last().map_or(0, |frame| frame.journal_start);
                self.commit_mutations(module, base)?;
            }
            OpCode::MakeFunction => {
                let fn_idx = self.read_u16(module)? as usize;
                let info =
                    module
                        .functions
                        .get(fn_idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 2,
                            reason: format!("function index {fn_idx} out of bounds"),
                        })?;
                let entry_ip = info.entry_ip as u32;
                let arity = info.params as u16;
                let is_async = info.is_async;
                self.push(Value::Function {
                    entry_ip,
                    arity,
                    is_async,
                })?;
            }
            OpCode::MakeClosure => {
                let fn_idx = self.read_u16(module)? as usize;
                let upvalue_count = self.read_u16(module)? as usize;
                let info =
                    module
                        .functions
                        .get(fn_idx)
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.ip - 4,
                            reason: format!("function index {fn_idx} out of bounds"),
                        })?;
                let entry_ip = info.entry_ip;
                let arity = info.params;
                let is_async = info.is_async;
                let mut upvalues = Vec::with_capacity(upvalue_count);
                for _ in 0..upvalue_count {
                    upvalues.push(Rc::new(RefCell::new(self.pop()?)));
                }
                upvalues.reverse();
                self.push(Value::closure(entry_ip, arity, upvalues, is_async))?;
            }
            OpCode::GetUpvalue => {
                let idx = self.read_u16(module)? as usize;
                let frame = self.upvalue_frames.last().cloned().flatten();
                let cell = frame
                    .and_then(|cells| cells.get(idx).cloned())
                    .ok_or(VmFault::StackUnderflow)?;
                let val = cell.borrow().clone();
                self.push(val)?;
            }
            OpCode::SetUpvalue => {
                let idx = self.read_u16(module)? as usize;
                let value = self.pop()?;
                let frame = self.upvalue_frames.last().cloned().flatten();
                let cell = frame
                    .and_then(|cells| cells.get(idx).cloned())
                    .ok_or(VmFault::StackUnderflow)?;
                *cell.borrow_mut() = value;
            }
            OpCode::Range => {
                let end = self.pop()?;
                let start = self.pop()?;
                match (start, end) {
                    (Value::Int(s), Value::Int(e)) => self.push(Value::range(s, e))?,
                    (a, b) => {
                        return Err(VmFault::TypeMismatch {
                            expected: "Int range bounds".to_string(),
                            actual: format!("{} and {}", a.type_name(), b.type_name()),
                        }
                        .into());
                    }
                }
            }
            OpCode::Len => {
                let value = self.pop()?;
                if value.is_failure() {
                    self.push(value)?;
                } else if let Value::Sequence(pipeline) = &value {
                    // Sequences drain (memoized) for `each` and friends;
                    // the global `len()` stays strict and rejects them.
                    let pipeline = Rc::clone(pipeline);
                    let items = self.sequence_items(module, &pipeline)?;
                    #[allow(clippy::cast_possible_wrap)]
                    let len = Value::Int(check_safe_int(items.len() as i64)?);
                    self.push(len)?;
                } else {
                    let len = length_of(&value)?;
                    self.push(len)?;
                }
            }
            OpCode::TypeIs => {
                let tag = self.pop()?;
                let value = self.pop()?;
                match tag {
                    Value::Type(tag) => self.push(Value::Bool(tag.matches(&value)))?,
                    Value::UserType(target_name) => {
                        let matches = match &value {
                            Value::Struct(inst) => {
                                let name = &inst.borrow().type_name;
                                name == target_name.as_str()
                                    || name
                                        .split_once('.')
                                        .is_some_and(|(parent, _)| parent == target_name.as_str())
                            }
                            _ => false,
                        };
                        self.push(Value::Bool(matches))?;
                    }
                    Value::Struct(target_inst) => {
                        let target_name = target_inst.borrow().type_name.clone();
                        let matches = match &value {
                            Value::Struct(inst) => inst.borrow().type_name == target_name,
                            _ => false,
                        };
                        self.push(Value::Bool(matches))?;
                    }
                    other => {
                        return Err(VmFault::TypeMismatch {
                            expected: "type value on the right of `is`".to_string(),
                            actual: other.type_name().to_string(),
                        }
                        .into());
                    }
                }
            }
            OpCode::TypeIsNullable => {
                let tag = self.pop()?;
                let value = self.pop()?;
                if matches!(value, Value::None) {
                    self.push(Value::Bool(true))?;
                } else {
                    match tag {
                        Value::Type(tag) => self.push(Value::Bool(tag.matches(&value)))?,
                        Value::UserType(target_name) => {
                            let matches = match &value {
                                Value::Struct(inst) => {
                                    let name = &inst.borrow().type_name;
                                    name == target_name.as_str()
                                        || name.split_once('.').is_some_and(|(parent, _)| {
                                            parent == target_name.as_str()
                                        })
                                }
                                _ => false,
                            };
                            self.push(Value::Bool(matches))?;
                        }
                        Value::Struct(target_inst) => {
                            let target_name = target_inst.borrow().type_name.clone();
                            let matches = match &value {
                                Value::Struct(inst) => inst.borrow().type_name == target_name,
                                _ => false,
                            };
                            self.push(Value::Bool(matches))?;
                        }
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "type value on the right of `is`".to_string(),
                                actual: other.type_name().to_string(),
                            }
                            .into());
                        }
                    }
                }
            }
            OpCode::IterGuard => {
                let value = self.pop()?;
                // Always push so `IterGuardEnd` stays balanced for values without a
                // collection identity (ranges, strings). The sentinel can never match a
                // real collection id, so the mutation guard ignores it.
                self.active_iterations
                    .push(collection_identity(&value).unwrap_or(usize::MAX));
                let revision = value.structural_revision();
                self.iteration_revisions
                    .push((value, revision, self.frames.len()));
            }
            OpCode::IterGuardEnd => {
                self.iteration_revisions.pop();
                self.active_iterations.pop();
            }
            OpCode::IterAt => {
                let mode = self.read_u8(module)?;
                let index = self.pop()?;
                let collection = self.pop()?;
                let projected = self.iter_at(module, &collection, &index, mode)?;
                self.push(projected)?;
            }
            OpCode::Fail => {
                let err_val = self.pop()?;
                let (message, payload) = match &err_val {
                    Value::String(s) => ((**s).clone(), Value::None),
                    Value::Failure(f) => (f.message.clone(), f.payload.clone()),
                    other => (other.to_string(), other.clone()),
                };
                let failure = Value::failure_with_payload(message, payload);
                self.handle_failure(failure)?;
            }
            OpCode::FillSelfCapture => {
                let _name_idx = self.read_u16(module)? as usize;
                let closure = self.pop()?;
                // The builder reserves exactly one `Unset` placeholder — the self capture —
                // and no other capture can hold the sentinel at creation time, because
                // captures snapshot already-initialized bindings.
                let cells = match &closure {
                    Value::Closure(closure) => closure.upvalues.clone(),
                    other => {
                        return Err(VmFault::TypeMismatch {
                            expected: "closure".to_string(),
                            actual: other.type_name().to_string(),
                        }
                        .into());
                    }
                };
                let filled = cells
                    .iter()
                    .find(|cell| matches!(&*cell.borrow(), Value::Unset))
                    .ok_or_else(|| VmFault::CorruptedBytecode {
                        offset: self.ip - 2,
                        reason: "FillSelfCapture without an Unset placeholder cell".to_string(),
                    })?;
                *filled.borrow_mut() = closure.clone();
                self.push(closure)?;
            }
            OpCode::CheckFailure => {
                // Statement boundary: an unconsumed recoverable Failure ends this path.
                if self.stack.last().is_some_and(Value::is_failure) {
                    let failure = self.pop()?;
                    self.handle_failure(failure)?;
                }
            }
            OpCode::OrElse => {
                let right = self.pop()?;
                let left = self.pop()?;
                if left.is_failure() {
                    self.push(right)?;
                } else {
                    self.push(left)?;
                }
            }
            OpCode::PushHandler => {
                let rel = self.read_i16(module)? as isize;
                let target = (self.ip as isize) + rel;
                if target < 0 || (target as usize) > module.code.len() {
                    return Err(VmFault::CorruptedBytecode {
                        offset: self.ip - 2,
                        reason: format!("PushHandler target {target} out of range"),
                    }
                    .into());
                }
                let mut handler =
                    HandlerFrame::new(target as usize, self.stack.len(), self.frames.len());
                handler.iteration_depth = self.iteration_revisions.len();
                handler.active_iteration_depth = self.active_iterations.len();
                self.handlers.push(handler);
            }
            OpCode::PopHandler => {
                self.handlers.pop();
            }
        }

        Ok(false)
    }

    pub(super) fn read_u8(&mut self, module: &BytecodeModule) -> Result<u8, VmFault> {
        if self.ip >= module.code.len() {
            return Err(VmFault::CorruptedBytecode {
                offset: self.ip,
                reason: "unexpected end of bytecode".to_string(),
            });
        }
        let b = module.code[self.ip];
        self.ip += 1;
        Ok(b)
    }

    pub(super) fn read_u16(&mut self, module: &BytecodeModule) -> Result<u16, VmFault> {
        if self.ip + 2 > module.code.len() {
            return Err(VmFault::CorruptedBytecode {
                offset: self.ip,
                reason: "truncated 16-bit operand".to_string(),
            });
        }
        let val = BigEndian::read_u16(&module.code[self.ip..self.ip + 2]);
        self.ip += 2;
        Ok(val)
    }

    pub(super) fn read_i16(&mut self, module: &BytecodeModule) -> Result<i16, VmFault> {
        if self.ip + 2 > module.code.len() {
            return Err(VmFault::CorruptedBytecode {
                offset: self.ip,
                reason: "truncated 16-bit jump offset".to_string(),
            });
        }
        let val = BigEndian::read_i16(&module.code[self.ip..self.ip + 2]);
        self.ip += 2;
        Ok(val)
    }

    /// Reads one `each` binding: mode `0` is the natural element, `1` the key/index and
    /// `2` the value.
    ///
    /// A `Dict` projects its key at the ordinal position for modes 0 and 1 and its value
    /// for mode 2; every other iterable projects its element for modes 0 and 2 and the
    /// ordinal `Int` for mode 1, which is what makes `each index, item in list` and
    /// `each key, value in dict` share one loop shape (auditoria IR-10).
    fn iter_at(
        &mut self,
        module: &BytecodeModule,
        collection: &Value,
        index: &Value,
        mode: u8,
    ) -> Result<Value, VmError> {
        let ordinal = match index {
            Value::Int(value) => *value,
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "Int iteration index".to_string(),
                    actual: other.type_name().to_string(),
                }
                .into());
            }
        };

        if let Value::Dict(dict) = collection {
            let entries = dict.borrow();
            let len = entries.len();
            #[allow(clippy::cast_possible_wrap)]
            let actual = if ordinal < 0 {
                (len as i64) + ordinal
            } else {
                ordinal
            };
            let position = usize::try_from(actual)
                .ok()
                .filter(|position| *position < len)
                .ok_or(VmFault::IndexOutOfRange {
                    index: ordinal,
                    len,
                })?;
            let (key, value) = &entries.entries()[position];
            return Ok(if mode == 2 {
                value.clone()
            } else {
                key.clone()
            });
        }

        if mode == 1 {
            // The two-name first binding is the positional index for every non-Dict
            // iterable.
            let len = match length_of(collection).map_err(|_| VmFault::TypeMismatch {
                expected: "iterable collection".to_string(),
                actual: collection.type_name().to_string(),
            })? {
                Value::Int(len) => usize::try_from(len).unwrap_or(0),
                _ => 0,
            };
            #[allow(clippy::cast_possible_wrap)]
            let actual = if ordinal < 0 {
                (len as i64) + ordinal
            } else {
                ordinal
            };
            let position = usize::try_from(actual)
                .ok()
                .filter(|position| *position < len)
                .ok_or(VmFault::IndexOutOfRange {
                    index: ordinal,
                    len,
                })?;
            #[allow(clippy::cast_possible_wrap)]
            return Ok(Value::Int(position as i64));
        }

        // Modes 0 and 2 reuse the canonical indexed read, so Lists, Strings, Bytes,
        // Ranges and Sequences keep their existing semantics.
        self.index_value(module, collection, ordinal)
    }

    /// Positional read for `IterAt`; strings share the `GetIndex` lookup helper.
    fn index_value(
        &mut self,
        module: &BytecodeModule,
        target: &Value,
        index: i64,
    ) -> Result<Value, VmError> {
        match target {
            Value::List(list) => {
                let len = list.borrow().len();
                #[allow(clippy::cast_possible_wrap)]
                let actual = if index < 0 {
                    (len as i64) + index
                } else {
                    index
                };
                let position = usize::try_from(actual)
                    .ok()
                    .filter(|position| *position < len)
                    .ok_or(VmFault::IndexOutOfRange { index, len })?;
                Ok(list.borrow()[position].clone())
            }
            Value::String(text) => Ok(Value::String(Rc::new(crate::value::string_char_at(
                text, index,
            )?))),
            Value::Bytes(bytes) => {
                let len = bytes.borrow().len();
                #[allow(clippy::cast_possible_wrap)]
                let actual = if index < 0 {
                    (len as i64) + index
                } else {
                    index
                };
                let position = usize::try_from(actual)
                    .ok()
                    .filter(|position| *position < len)
                    .ok_or(VmFault::IndexOutOfRange { index, len })?;
                Ok(Value::Byte(bytes.borrow()[position]))
            }
            Value::Range(r) => {
                let len = usize::try_from((r.end - r.start).max(0)).unwrap_or(0);
                #[allow(clippy::cast_possible_wrap)]
                let actual = if index < 0 {
                    (len as i64) + index
                } else {
                    index
                };
                let position = usize::try_from(actual)
                    .ok()
                    .filter(|position| *position < len)
                    .ok_or(VmFault::IndexOutOfRange { index, len })?;
                Ok(Value::Int(r.start + position as i64))
            }
            Value::Sequence(pipeline) => {
                let items = self.sequence_items(module, pipeline)?;
                let len = items.len();
                #[allow(clippy::cast_possible_wrap)]
                let actual = if index < 0 {
                    (len as i64) + index
                } else {
                    index
                };
                let position = usize::try_from(actual)
                    .ok()
                    .filter(|position| *position < len)
                    .ok_or(VmFault::IndexOutOfRange { index, len })?;
                Ok(items[position].clone())
            }
            other => Err(VmFault::TypeMismatch {
                expected: "iterable collection".to_string(),
                actual: other.type_name().to_string(),
            }
            .into()),
        }
    }
}
