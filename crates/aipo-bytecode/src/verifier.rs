//! Bytecode verifier checking structural invariants, bounds, and jump targets.

use crate::module::{AIBC_MAGIC, AIBC_VERSION, BytecodeModule, FunctionInfo};
use crate::opcode::OpCode;
use byteorder::{BigEndian, ByteOrder};
use std::collections::HashSet;

/// Verifier enforcing bytecode integrity before execution.
pub struct BytecodeVerifier;

impl BytecodeVerifier {
    /// Verifies that a bytecode module is structurally sound and safe to execute.
    ///
    /// # Errors
    /// Returns a list of verification failure messages if any invariant is violated.
    pub fn verify(module: &BytecodeModule) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if module.magic != AIBC_MAGIC {
            errors.push(format!("invalid magic header: {:?}", module.magic));
        }

        if module.version != AIBC_VERSION {
            errors.push(format!("unsupported bytecode version: {}", module.version));
        }

        // Pass 1: Collect valid instruction boundary offsets
        let mut valid_offsets = HashSet::new();
        let mut cursor = 0;
        while cursor < module.code.len() {
            valid_offsets.insert(cursor);
            let byte = module.code[cursor];
            let opcode = match OpCode::try_from(byte) {
                Ok(op) => op,
                Err(b) => {
                    errors.push(format!(
                        "invalid opcode byte 0x{:02x} at offset {}",
                        b, cursor
                    ));
                    cursor += 1;
                    continue;
                }
            };

            let size = Self::instruction_size(opcode, &module.code[cursor..]);
            if cursor + size > module.code.len() {
                errors.push(format!(
                    "instruction {:?} at offset {} truncated (expected {} bytes, remaining {})",
                    opcode,
                    cursor,
                    size,
                    module.code.len() - cursor
                ));
                break;
            }
            cursor += size;
        }
        valid_offsets.insert(module.code.len()); // Allow jump to EOF / end

        // Pass 2: Verify operand bounds and jump targets
        cursor = 0;
        while cursor < module.code.len() {
            let byte = module.code[cursor];
            let opcode = match OpCode::try_from(byte) {
                Ok(op) => op,
                Err(_) => {
                    cursor += 1;
                    continue;
                }
            };

            match opcode {
                OpCode::Constant => {
                    if cursor + 3 <= module.code.len() {
                        let idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if idx >= module.constants.len() {
                            errors.push(format!(
                                "OpConstant index {} out of bounds (constants len: {})",
                                idx,
                                module.constants.len()
                            ));
                        }
                    }
                    cursor += 3;
                }
                OpCode::GetGlobal
                | OpCode::SetGlobal
                | OpCode::GetField
                | OpCode::SetField
                | OpCode::IsVariant => {
                    if cursor + 3 <= module.code.len() {
                        let idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if idx >= module.names.len() {
                            errors.push(format!(
                                "name index {} out of bounds (names len: {})",
                                idx,
                                module.names.len()
                            ));
                        }
                    }
                    cursor += 3;
                }
                OpCode::Jump | OpCode::JumpIfFalse | OpCode::PushHandler => {
                    if cursor + 3 <= module.code.len() {
                        let rel =
                            BigEndian::read_i16(&module.code[cursor + 1..cursor + 3]) as isize;
                        let target = (cursor as isize) + 3 + rel;
                        if target < 0 || (target as usize) > module.code.len() {
                            errors.push(format!(
                                "jump offset {} from {} targets out-of-bounds address {}",
                                rel, cursor, target
                            ));
                        } else if !valid_offsets.contains(&(target as usize)) {
                            errors.push(format!(
                                "jump offset {} from {} targets invalid non-instruction boundary {}",
                                rel, cursor, target
                            ));
                        }
                    }
                    cursor += 3;
                }
                OpCode::IterAt => {
                    if cursor + 2 <= module.code.len() {
                        let mode = module.code[cursor + 1];
                        if mode > 2 {
                            errors.push(format!(
                                "IterAt mode {mode} must be 0 (primary), 1 (key) or 2 (value) at offset {cursor}"
                            ));
                        }
                    }
                    cursor += 2;
                }
                OpCode::JumpIfSetLocal => {
                    if cursor + 5 <= module.code.len() {
                        let slot =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if let Some(func) = Self::enclosing_function(module, cursor) {
                            let limit = func.params + func.locals;
                            if slot >= limit {
                                errors.push(format!(
                                    "JumpIfSetLocal slot {slot} out of bounds for function '{}' (limit {limit}) at offset {cursor}",
                                    func.name
                                ));
                            }
                        }
                        let rel =
                            BigEndian::read_i16(&module.code[cursor + 3..cursor + 5]) as isize;
                        let target = (cursor as isize) + 5 + rel;
                        if target < 0 || (target as usize) > module.code.len() {
                            errors.push(format!(
                                "jump offset {} from {} targets out-of-bounds address {}",
                                rel, cursor, target
                            ));
                        } else if !valid_offsets.contains(&(target as usize)) {
                            errors.push(format!(
                                "jump offset {} from {} targets invalid non-instruction boundary {}",
                                rel, cursor, target
                            ));
                        }
                    }
                    cursor += 5;
                }
                OpCode::BuildStruct => {
                    if cursor + 6 <= module.code.len() {
                        let type_idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if type_idx >= module.names.len() {
                            errors.push(format!(
                                "BuildStruct type index {} out of bounds (names len: {})",
                                type_idx,
                                module.names.len()
                            ));
                        }
                        let field_count =
                            BigEndian::read_u16(&module.code[cursor + 3..cursor + 5]) as usize;
                        if let Some(type_name) = module.names.get(type_idx)
                            && let Some(info) = module.structs.iter().find(|s| s.name == *type_name)
                            && field_count != info.fields.len()
                        {
                            errors.push(format!(
                                        "BuildStruct for {type_name} expected {} fields, got {field_count} at offset {cursor}",
                                        info.fields.len()
                                    ));
                        }
                        let defer_fixed = module.code[cursor + 5];
                        if defer_fixed > 1 {
                            errors.push(format!(
                                "BuildStruct defer_fixed flag {defer_fixed} must be 0 or 1 at {cursor}"
                            ));
                        }
                    }
                    cursor += 6;
                }
                OpCode::MakeFunction => {
                    if cursor + 3 <= module.code.len() {
                        let idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if idx >= module.functions.len() {
                            errors.push(format!(
                                "MakeFunction index {} out of bounds (functions len: {})",
                                idx,
                                module.functions.len()
                            ));
                        }
                    }
                    cursor += 3;
                }
                OpCode::MakeClosure => {
                    if cursor + 5 <= module.code.len() {
                        let idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if idx >= module.functions.len() {
                            errors.push(format!(
                                "MakeClosure index {} out of bounds (functions len: {})",
                                idx,
                                module.functions.len()
                            ));
                        }
                    }
                    cursor += 5;
                }
                OpCode::Call => cursor += 2,
                OpCode::FillSelfCapture => {
                    if cursor + 3 <= module.code.len() {
                        let idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if idx >= module.names.len() {
                            errors.push(format!(
                                "FillSelfCapture name index {} out of bounds (names len: {})",
                                idx,
                                module.names.len()
                            ));
                        }
                    }
                    cursor += 3;
                }
                OpCode::AssertInvariant => {
                    if cursor + 3 <= module.code.len() {
                        let idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if idx >= module.names.len() {
                            errors.push(format!(
                                "AssertInvariant type index {} out of bounds (names len: {})",
                                idx,
                                module.names.len()
                            ));
                        }
                    }
                    cursor += 3;
                }
                OpCode::AssertContract => {
                    if cursor + 6 <= module.code.len() {
                        let type_idx =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if type_idx >= module.names.len() {
                            errors.push(format!(
                                "AssertContract type index {} out of bounds (names len: {})",
                                type_idx,
                                module.names.len()
                            ));
                        }
                        let nullable = module.code[cursor + 3];
                        if nullable > 1 {
                            errors.push(format!(
                                "AssertContract nullable flag {nullable} must be 0 or 1 at {cursor}"
                            ));
                        }
                        let position_idx =
                            BigEndian::read_u16(&module.code[cursor + 4..cursor + 6]) as usize;
                        if position_idx >= module.names.len() {
                            errors.push(format!(
                                "AssertContract position index {} out of bounds (names len: {})",
                                position_idx,
                                module.names.len()
                            ));
                        }
                        if cursor + 7 <= module.code.len() {
                            let operations = module.code[cursor + 6] as usize;
                            for index in 0..operations {
                                let base = cursor + 7 + index * 3;
                                if base + 3 <= module.code.len() {
                                    let name_idx =
                                        BigEndian::read_u16(&module.code[base..base + 2]) as usize;
                                    if name_idx >= module.names.len() {
                                        errors.push(format!(
                                            "AssertContract operation name index {name_idx} out of bounds (names len: {})",
                                            module.names.len()
                                        ));
                                    }
                                }
                            }
                        }
                    }
                    cursor += Self::assert_contract_size(&module.code[cursor..]);
                }
                OpCode::GetLocal | OpCode::SetLocal => {
                    if cursor + 3 <= module.code.len() {
                        let slot =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if let Some(func) = Self::enclosing_function(module, cursor) {
                            let limit = func.params + func.locals;
                            if slot >= limit {
                                errors.push(format!(
                                    "{} slot {slot} out of bounds for function '{}' (limit {limit}) at offset {cursor}",
                                    if opcode == OpCode::GetLocal { "GetLocal" } else { "SetLocal" },
                                    func.name
                                ));
                            }
                        }
                    }
                    cursor += 3;
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
                    if let Some(func) = Self::enclosing_function(module, cursor) {
                        let limit = func.params + func.locals;
                        if slot >= limit {
                            errors.push(format!(
                                "GetLocal{slot} slot {slot} out of bounds for function '{}' (limit {limit}) at offset {cursor}",
                                func.name
                            ));
                        }
                    }
                    cursor += 1;
                }
                OpCode::GetUpvalue | OpCode::SetUpvalue => {
                    if cursor + 3 <= module.code.len() {
                        let slot =
                            BigEndian::read_u16(&module.code[cursor + 1..cursor + 3]) as usize;
                        if let Some(func) = Self::enclosing_function(module, cursor)
                            && slot >= func.upvalues
                        {
                            errors.push(format!(
                                    "{} slot {slot} out of bounds for function '{}' (limit {}) at offset {cursor}",
                                    if opcode == OpCode::GetUpvalue { "GetUpvalue" } else { "SetUpvalue" },
                                    func.name,
                                    func.upvalues
                                ));
                        }
                    }
                    cursor += 3;
                }
                OpCode::BuildList | OpCode::BuildDict => {
                    cursor += 3;
                }
                _ => cursor += 1,
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Finds the enclosing compiled function metadata for an instruction offset.
    fn enclosing_function(module: &BytecodeModule, cursor: usize) -> Option<&FunctionInfo> {
        module.functions.iter().rev().find(|f| f.entry_ip <= cursor)
    }

    /// Size of an `AssertContract` instruction: the opcode, five fixed operands and the operation
    /// count, plus three bytes per required operation (name index + arity).
    fn assert_contract_size(slice: &[u8]) -> usize {
        let operations = slice.get(6).copied().unwrap_or(0) as usize;
        7 + 3 * operations
    }

    /// Byte length of a decoded instruction, including variable contract operands.
    pub fn instruction_size(opcode: OpCode, slice: &[u8]) -> usize {
        match opcode {
            OpCode::Constant
            | OpCode::GetLocal
            | OpCode::SetLocal
            | OpCode::GetGlobal
            | OpCode::SetGlobal
            | OpCode::Jump
            | OpCode::JumpIfFalse
            | OpCode::PushHandler
            | OpCode::GetField
            | OpCode::SetField
            | OpCode::BuildList
            | OpCode::BuildDict
            | OpCode::GetUpvalue
            | OpCode::SetUpvalue
            | OpCode::MakeFunction
            | OpCode::AssertInvariant
            | OpCode::IsVariant
            | OpCode::FillSelfCapture => 3,
            // The contract operand list is variable length, so it is sized from the slice by
            // `assert_contract_size` rather than from this table.
            OpCode::JumpIfSetLocal => 5,
            OpCode::AssertContract => Self::assert_contract_size(slice),
            OpCode::Call => 2,
            OpCode::IterAt => 2,
            OpCode::BuildStruct => 6,
            OpCode::MakeClosure => 5,
            _ => 1,
        }
    }
}
