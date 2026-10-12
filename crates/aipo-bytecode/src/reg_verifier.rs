//! Verification of register operands, definite assignment and control-flow scopes.

use crate::{Constant, RegCompiledFunction, RegCompiledModule, RegInstruction, RegOpCode};
use std::collections::VecDeque;

#[derive(Clone, PartialEq, Eq)]
struct State {
    initialized: [bool; 256],
    handlers: Vec<(usize, usize, usize)>,
    guards: usize,
}

/// Verifies register modules before they reach an execution engine.
pub struct RegVerifier;

impl RegVerifier {
    /// Checks every function, including unreachable instruction operands.
    ///
    /// # Errors
    /// Returns the first invalid operand, scope or uninitialized read.
    pub fn verify(module: &RegCompiledModule) -> Result<(), String> {
        Self::verify_function(&module.top_level, module.functions.len())?;
        for function in &module.functions {
            Self::verify_function(function, module.functions.len())?;
        }
        Ok(())
    }

    /// Verifies one activation, treating parameters as initialized.
    ///
    /// # Errors
    /// Returns a diagnostic containing the function and instruction offset.
    pub fn verify_function(function: &RegCompiledFunction, functions: usize) -> Result<(), String> {
        if function.is_async
            || !(1..=256).contains(&function.num_registers)
            || function.arity > function.num_registers
        {
            return Err(format!("{}: invalid register metadata", function.name));
        }
        Self::verify_code(
            &function.instructions,
            &function.constants,
            functions,
            function.num_registers,
            function.arity,
            false,
        )
        .map_err(|error| format!("{}: {error}", function.name))
    }

    /// Verifies host-supplied instructions. All host registers are initialized.
    ///
    /// # Errors
    /// Rejects invalid operands and inconsistent scopes before executing any instruction.
    pub fn verify_raw(
        code: &[RegInstruction],
        constants: &[Constant],
        functions: usize,
    ) -> Result<(), String> {
        Self::verify_code(code, constants, functions, 256, 256, true)
    }

    fn verify_code(
        code: &[RegInstruction],
        constants: &[Constant],
        functions: usize,
        registers: usize,
        arity: usize,
        allow_empty: bool,
    ) -> Result<(), String> {
        use RegOpCode::*;
        if code.is_empty() {
            return if allow_empty {
                Ok(())
            } else {
                Err("empty function".into())
            };
        }
        let target = |pc: usize, inst: RegInstruction| {
            (pc + 1)
                .checked_add_signed(inst.sbx() as isize)
                .filter(|next| *next < code.len())
                .ok_or_else(|| format!("instruction {pc}: jump outside function"))
        };
        for constant in constants {
            match constant {
                Constant::Int(value)
                    if !(-9_007_199_254_740_991..=9_007_199_254_740_991).contains(value) =>
                {
                    return Err("constant exceeds Aipo safe integer range".into());
                }
                Constant::Float(value) if !value.is_finite() => {
                    return Err("non-finite constant".into());
                }
                _ => {}
            }
        }
        let mut operands = Vec::with_capacity(code.len());
        for (pc, &inst) in code.iter().enumerate() {
            let op = inst
                .opcode()
                .ok_or_else(|| format!("instruction {pc}: unknown opcode"))?;
            let (a, b, c) = (inst.a() as usize, inst.b() as usize, inst.c() as usize);
            let mut reads = Vec::new();
            let mut writes = Vec::new();
            let mut string_constants = Vec::new();
            match op {
                Nop | Jump | PopHandler | IterGuardEnd => {}
                LoadConst => {
                    if inst.bx() as usize >= constants.len() {
                        return Err(format!("instruction {pc}: constant outside pool"));
                    }
                    writes.push(a);
                }
                LoadInt | LoadNil | LoadBool | LoadUnset => {
                    writes.push(a);
                }
                MakeFunction => {
                    if inst.bx() as usize >= functions {
                        return Err(format!("instruction {pc}: function outside table"));
                    }
                    writes.push(a);
                }
                GetGlobal => {
                    string_constants.push(inst.bx() as usize);
                    writes.push(a);
                }
                SetGlobal => {
                    string_constants.push(inst.bx() as usize);
                    reads.push(a);
                }
                GetField | IsVariant => {
                    string_constants.push(inst.bx() as usize);
                    reads.push(a);
                    writes.push(a);
                }
                SetField => {
                    string_constants.push(b);
                    reads.extend([a, c]);
                }
                AssertContract | AssertContractNullable => {
                    string_constants.extend([inst.bx() as usize, inst.bx() as usize + 1]);
                    reads.push(a);
                }
                Move | Neg | Not | Len | CloneStruct => {
                    reads.push(b);
                    writes.push(a);
                }
                Add | Sub | Mul | Div | IntDiv | Mod | Equal | NotEqual | Less | LessEqual
                | Greater | GreaterEqual | GetIndex | Range | TypeIs | TypeIsNullable
                | IterPrimary | IterKey | IterValue => {
                    reads.extend([b, c]);
                    writes.push(a);
                }
                IterAt => {
                    if c & 3 == 3 {
                        return Err(format!("instruction {pc}: invalid iteration projection"));
                    }
                    reads.extend([b, c >> 2]);
                    writes.push(a);
                }
                CallPipe => {
                    reads.extend([a, b, c]);
                    writes.push(a);
                }
                SetIndex => {
                    reads.extend([a, b, c]);
                }
                Call => {
                    if c > 1 {
                        return Err(format!("instruction {pc}: unsupported result count"));
                    }
                    reads.extend(a..=a + b);
                    writes.push(a);
                }
                NewList | NewDict => {
                    let count = if op == NewDict { c * 2 } else { c };
                    if b + count > registers {
                        return Err(format!(
                            "instruction {pc}: collection register window outside activation"
                        ));
                    }
                    reads.extend(b..b + count);
                    writes.push(a);
                }
                NewStruct => {
                    string_constants.push(b);
                    if a + c > registers {
                        return Err(format!(
                            "instruction {pc}: struct register window outside activation"
                        ));
                    }
                    reads.extend(a..a + c);
                    writes.push(a);
                }
                Return | JumpIfTrue | JumpIfFalse | JumpIfSetLocal | IterGuard => reads.push(a),
                Fail | PropagateFailure => {
                    reads.push(a);
                    writes.push(a);
                }
                PushHandler => {
                    if a >= registers {
                        return Err(format!("instruction {pc}: invalid handler register"));
                    }
                }
                AssertInvariant => {
                    string_constants.push(inst.bx() as usize);
                    reads.push(a);
                }
                SealStruct | FillSelfCapture | CheckMutations => {
                    reads.push(a);
                }
                MakeClosure => {
                    if b >= functions {
                        return Err(format!("instruction {pc}: function outside table"));
                    }
                    writes.push(a);
                }
                GetUpvalue => {
                    writes.push(a);
                }
                SetUpvalue => {
                    reads.push(a);
                }
                Await => {
                    reads.push(a);
                    writes.push(a);
                }
            }
            for index in string_constants {
                if !matches!(constants.get(index), Some(Constant::String(_))) {
                    return Err(format!(
                        "instruction {pc}: expected string constant at {index}"
                    ));
                }
            }
            if reads.iter().chain(&writes).any(|&reg| reg >= registers) {
                return Err(format!("instruction {pc}: register outside activation"));
            }
            if matches!(
                op,
                Jump | JumpIfTrue | JumpIfFalse | JumpIfSetLocal | PushHandler
            ) {
                target(pc, inst)?;
            }
            operands.push((reads, writes));
        }
        let mut initial = State {
            initialized: [false; 256],
            handlers: Vec::new(),
            guards: 0,
        };
        initial.initialized[..arity].fill(true);
        let mut states = vec![None; code.len()];
        states[0] = Some(initial);
        let mut queue = VecDeque::from([0]);
        while let Some(pc) = queue.pop_front() {
            let mut state = states[pc].clone().expect("queued reachable state");
            let inst = code[pc];
            let op = inst.opcode().expect("validated opcode");
            for &reg in &operands[pc].1 {
                state.initialized[reg] = true;
            }
            let mut edges = Vec::new();
            match op {
                PushHandler => {
                    // The exceptional edge restores the scope at handler installation.
                    let mut failure = state.clone();
                    failure.initialized[inst.a() as usize] = true;
                    edges.push((target(pc, inst)?, failure));
                    state
                        .handlers
                        .push((target(pc, inst)?, inst.a() as usize, state.guards));
                }
                PopHandler => {
                    state
                        .handlers
                        .pop()
                        .ok_or_else(|| format!("instruction {pc}: handler underflow"))?;
                }
                IterGuard => state.guards += 1,
                IterGuardEnd => {
                    state.guards = state
                        .guards
                        .checked_sub(1)
                        .ok_or_else(|| format!("instruction {pc}: iteration scope underflow"))?;
                }
                _ => {}
            }
            if matches!(op, Jump | JumpIfTrue | JumpIfFalse | JumpIfSetLocal) {
                edges.push((target(pc, inst)?, state.clone()));
            }
            if !matches!(op, Jump | Return | Fail) {
                if pc + 1 == code.len() {
                    return Err(format!(
                        "instruction {pc}: reachable fallthrough without return"
                    ));
                }
                edges.push((pc + 1, state));
            }
            for (next, incoming) in edges {
                match &mut states[next] {
                    None => {
                        states[next] = Some(incoming);
                        queue.push_back(next);
                    }
                    Some(existing) => {
                        if existing.handlers != incoming.handlers
                            || existing.guards != incoming.guards
                        {
                            return Err(format!(
                                "instruction {next}: inconsistent handler or iteration scopes"
                            ));
                        }
                        let mut changed = false;
                        for (old, new) in existing.initialized.iter_mut().zip(incoming.initialized)
                        {
                            if *old && !new {
                                *old = false;
                                changed = true;
                            }
                        }
                        if changed {
                            queue.push_back(next);
                        }
                    }
                }
            }
        }
        for (pc, state) in states.iter().enumerate() {
            if let Some(state) = state {
                for &reg in &operands[pc].0 {
                    if !state.initialized[reg] {
                        return Err(format!(
                            "instruction {pc}: register {reg} may be uninitialized"
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}
