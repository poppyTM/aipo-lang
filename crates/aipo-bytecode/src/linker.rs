//! Relocates independent compilation units without executing old entry scripts.
use crate::{BytecodeModule, BytecodeVerifier, OpCode};

/// Appends `unit` atomically and returns its entry offset. Old function values stay valid.
///
/// # Errors
/// Rejects invalid bytecode or pool/code offsets that exceed the binary format.
pub fn append_unit(
    linked: &mut BytecodeModule,
    unit: &BytecodeModule,
) -> Result<usize, Vec<String>> {
    BytecodeVerifier::verify(unit)?;
    let mut candidate = linked.clone();
    let base = candidate.code.len();
    let function_base = candidate.functions.len();
    let mut names = Vec::with_capacity(unit.names.len());
    for name in &unit.names {
        let index = candidate
            .names
            .iter()
            .position(|old| old == name)
            .unwrap_or_else(|| {
                candidate.names.push(name.clone());
                candidate.names.len() - 1
            });
        names.push(index);
    }
    let mut constants = Vec::with_capacity(unit.constants.len());
    for constant in &unit.constants {
        let index = candidate
            .constants
            .iter()
            .position(|old| old == constant)
            .unwrap_or_else(|| {
                candidate.constants.push(constant.clone());
                candidate.constants.len() - 1
            });
        constants.push(index);
    }
    if candidate.names.len() > 65536
        || candidate.constants.len() > 65536
        || function_base + unit.functions.len() > 65536
        || base
            .checked_add(unit.code.len())
            .is_none_or(|size| size > u32::MAX as usize)
    {
        return Err(vec!["linked module exceeds bytecode format limits".into()]);
    }
    let mut code = unit.code.clone();
    let mut pc = 0;
    while pc < code.len() {
        let op = OpCode::try_from(code[pc]).expect("verified opcode");
        let size = BytecodeVerifier::instruction_size(op, &code[pc..]);
        let mut offsets = Vec::new();
        match op {
            OpCode::Constant => offsets.push((1, 0)),
            OpCode::MakeFunction | OpCode::MakeClosure => offsets.push((1, 2)),
            OpCode::GetGlobal
            | OpCode::SetGlobal
            | OpCode::GetField
            | OpCode::SetField
            | OpCode::IsVariant
            | OpCode::BuildStruct
            | OpCode::AssertInvariant
            | OpCode::FillSelfCapture => offsets.push((1, 1)),
            OpCode::AssertContract => {
                offsets.extend([(1, 1), (4, 1)]);
                for operation in 0..code[pc + 6] as usize {
                    offsets.push((7 + operation * 3, 1));
                }
            }
            _ => {}
        }
        for (offset, pool) in offsets {
            let index = u16::from_be_bytes([code[pc + offset], code[pc + offset + 1]]) as usize;
            let relocated = match pool {
                0 => constants[index],
                1 => names[index],
                _ => index + function_base,
            };
            code[pc + offset..pc + offset + 2].copy_from_slice(&(relocated as u16).to_be_bytes());
        }
        pc += size;
    }
    candidate.code.extend(code);
    candidate.spans.extend(
        unit.spans
            .iter()
            .map(|(offset, span)| (offset + base, *span)),
    );
    for function in &unit.functions {
        let mut function = function.clone();
        function.entry_ip += base;
        candidate.functions.push(function);
    }
    for decl in &unit.structs {
        if let Some(old) = candidate
            .structs
            .iter_mut()
            .find(|old| old.name == decl.name)
        {
            *old = decl.clone();
        } else {
            candidate.structs.push(decl.clone());
        }
    }
    BytecodeVerifier::verify(&candidate)?;
    *linked = candidate;
    Ok(base)
}
