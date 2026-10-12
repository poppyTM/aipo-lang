//! Conservative optimization passes over target-neutral Core IR.
//!
//! Every pass rewrites one function's instruction stream and returns an
//! index map so absolute jump targets can be re-noted. Removing an instruction
//! renumbers every target after it, so the passes are only correct together
//! with `remap_jump_targets`; that function is applied after each round.
//!
//! The passes never change observable behaviour: constant folding evaluates
//! only total subexpressions, peephole removes no-op shapes, and dead-code
//! elimination keeps every address a jump can reach.

use crate::{CoreConstant, CoreFunction, CoreInst, CoreModule};
use aipo_ast::{BinaryOp, UnaryOp};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

/// Normalizes `text` to NFC, the form canon makes an invariant of `String`.
fn nfc(text: &str) -> String {
    text.nfc().collect()
}

/// Maximum fixpoint rounds before keeping the current stream.
const MAX_FIXPOINT_ROUNDS: u8 = 8;

/// Maps an instruction index in the previous stream to its index in the new one.
type IndexMap = Vec<Option<usize>>;

/// Runs every optimization pass over the module to a fixpoint.
///
/// Each function is optimized independently; the module's struct declarations
/// are carried through untouched.
#[must_use]
pub fn optimize(module: &CoreModule) -> CoreModule {
    CoreModule {
        functions: module.functions.iter().map(optimize_function).collect(),
        top_level: optimize_function(&module.top_level),
        structs: module.structs.clone(),
        span: module.span,
    }
}

/// Runs every pass over one function until the stream stops shrinking.
fn optimize_function(function: &CoreFunction) -> CoreFunction {
    let mut current = function.clone();
    for _ in 0..MAX_FIXPOINT_ROUNDS {
        let before = current.instructions.len();

        let folded = fold_constants(&current.instructions);
        let (after_fold, map_fold) = folded;
        current.instructions = remap_jump_targets(after_fold, &map_fold);

        let peeped = peephole(&current.instructions);
        let (after_peep, map_peep) = peeped;
        current.instructions = remap_jump_targets(after_peep, &map_peep);

        let dead = eliminate_dead_code(&current.instructions);
        let (after_dead, map_dead) = dead;
        current.instructions = remap_jump_targets(after_dead, &map_dead);

        if current.instructions.len() == before {
            break;
        }
    }
    current
}

/// Folds `Constant, Constant, Binary` and `Constant, Unary` into one constant.
///
/// Returns the new stream and the map from old to new indices; a folded triple
/// contributes `Some` for its first instruction and `None` for the rest.
fn fold_constants(instructions: &[CoreInst]) -> (Vec<CoreInst>, IndexMap) {
    let mut out = Vec::with_capacity(instructions.len());
    let mut map: IndexMap = vec![None; instructions.len()];
    let mut index = 0;

    while index < instructions.len() {
        if let Some((folded, span, consumed)) = try_fold(&instructions[index..]) {
            map[index] = Some(out.len());
            out.push(CoreInst::Constant(folded, span));
            for offset in 1..consumed {
                map[index + offset] = None;
            }
            index += consumed;
            continue;
        }

        map[index] = Some(out.len());
        out.push(instructions[index].clone());
        index += 1;
    }
    (out, map)
}

/// Attempts to fold a constant expression at the head of `window`.
///
/// Returns the folded value, its span, and how many instructions it consumed.
fn try_fold(window: &[CoreInst]) -> Option<(CoreConstant, aipo_source::SourceSpan, usize)> {
    if let [
        CoreInst::Constant(left, _),
        CoreInst::Constant(right, _),
        CoreInst::Binary(op, span),
        ..,
    ] = window
    {
        if let Some(folded) = fold_binary(*op, left, right) {
            return Some((folded, *span, 3));
        }
        return None;
    }

    if let [CoreInst::Constant(inner, _), CoreInst::Unary(op, span), ..] = window
        && let Some(folded) = fold_unary(*op, inner)
    {
        return Some((folded, *span, 2));
    }
    None
}

/// Evaluates a binary operator on two constants, or `None` when it is not total.
///
/// Division, modulo, comparison and logic are excluded on purpose: traps, NaN,
/// `Failure` propagation and `none` handling are defined by the runtime, not by
/// the folder.
fn fold_binary(op: BinaryOp, left: &CoreConstant, right: &CoreConstant) -> Option<CoreConstant> {
    use BinaryOp::{Add, Mul, Sub};
    match (op, left, right) {
        (Add, CoreConstant::Int(left), CoreConstant::Int(right)) => {
            left.checked_add(*right).map(CoreConstant::Int)
        }
        (Sub, CoreConstant::Int(left), CoreConstant::Int(right)) => {
            left.checked_sub(*right).map(CoreConstant::Int)
        }
        (Mul, CoreConstant::Int(left), CoreConstant::Int(right)) => {
            left.checked_mul(*right).map(CoreConstant::Int)
        }
        (Add, CoreConstant::Float(left), CoreConstant::Float(right)) => {
            Some(CoreConstant::Float(left + right))
        }
        (Sub, CoreConstant::Float(left), CoreConstant::Float(right)) => {
            Some(CoreConstant::Float(left - right))
        }
        (Mul, CoreConstant::Float(left), CoreConstant::Float(right)) => {
            Some(CoreConstant::Float(left * right))
        }
        (Add, CoreConstant::String(left), CoreConstant::String(right)) => {
            // NFC is an invariant of `String`, and concatenation is one of the boundaries that
            // can join a base character with a combining mark. The folder must compose exactly
            // like the runtime's `Add`, or an optimized build would store `"e\u{0301}"` where
            // an unoptimized one stores `"é"` — a value change that also shifts `len`.
            Some(CoreConstant::String(nfc(&format!("{left}{right}"))))
        }
        _ => None,
    }
}

/// Evaluates a unary operator on one constant, or `None` when it is not total.
fn fold_unary(op: UnaryOp, inner: &CoreConstant) -> Option<CoreConstant> {
    use UnaryOp::{Neg, Not, Pos};
    match (op, inner) {
        (Neg, CoreConstant::Int(value)) => value.checked_neg().map(CoreConstant::Int),
        (Neg, CoreConstant::Float(value)) => Some(CoreConstant::Float(-value)),
        (Pos, CoreConstant::Int(value)) => Some(CoreConstant::Int(*value)),
        (Pos, CoreConstant::Float(value)) => Some(CoreConstant::Float(*value)),
        (Not, CoreConstant::Bool(value)) => Some(CoreConstant::Bool(!value)),
        _ => None,
    }
}

/// Removes instruction shapes with no observable effect.
///
/// - A `Jump` into the immediately following instruction, which a fall-through
///   already performs.
/// - A `JumpIfFalse` on a folded `Bool`, which resolves the branch statically.
/// - A constant that the next instruction pops, which never escapes.
/// - `Dup` immediately followed by `Pop`, which nets nothing.
///
/// Handler push/pop pairs, guards and contract assertions are never touched:
/// unwinding depends on them even when the visible effect is nil.
fn peephole(instructions: &[CoreInst]) -> (Vec<CoreInst>, IndexMap) {
    let mut out = Vec::with_capacity(instructions.len());
    let mut map: IndexMap = vec![None; instructions.len()];
    let mut index = 0;

    while index < instructions.len() {
        let window = &instructions[index..];

        // A jump to the next instruction is a fall-through already.
        if let [CoreInst::Jump(target, _), ..] = window
            && usize::try_from(*target).ok() == Some(index + 1)
        {
            index += 1;
            continue;
        }

        // A statically known branch resolves before execution.
        if let [
            CoreInst::Constant(CoreConstant::Bool(flag), _),
            CoreInst::JumpIfFalse(target, span),
            ..,
        ] = window
        {
            if *flag {
                // `true` never takes the branch: keep the condition it tested.
                map[index] = Some(out.len());
                out.push(CoreInst::Constant(CoreConstant::Bool(true), *span));
            } else {
                // `false` always takes it.
                map[index] = Some(out.len());
                out.push(CoreInst::Jump(*target, *span));
            }
            map[index + 1] = None;
            index += 2;
            continue;
        }

        // A constant popped right after it never escapes the stack.
        if let [CoreInst::Constant(_, _), CoreInst::Pop(_), ..] = window {
            index += 2;
            continue;
        }

        // `Dup` then `Pop` nets nothing.
        if let [CoreInst::Dup(_), CoreInst::Pop(_), ..] = window {
            index += 2;
            continue;
        }

        map[index] = Some(out.len());
        out.push(instructions[index].clone());
        index += 1;
    }
    (out, map)
}

/// Removes instructions no control path can reach.
///
/// A statement after an unconditional exit (`Return`, `Jump`, `Fail`) is dead
/// until some jump lands on it. Targets that survive from the previous stream
/// become the new live entries; anything the pass cannot prove dead is kept,
/// because dropping reachable code is unsound.
fn eliminate_dead_code(instructions: &[CoreInst]) -> (Vec<CoreInst>, IndexMap) {
    let live_entries = live_entry_indices(instructions);

    let mut out = Vec::with_capacity(instructions.len());
    let mut map: IndexMap = vec![None; instructions.len()];
    let mut dead = false;

    for (index, inst) in instructions.iter().enumerate() {
        if live_entries.contains(&index) {
            dead = false;
        }
        if dead {
            continue;
        }
        map[index] = Some(out.len());
        out.push(inst.clone());
        if is_unconditional_exit(inst) {
            dead = true;
        }
    }
    (out, map)
}

/// Collects the indices a jump can transfer control to.
fn live_entry_indices(instructions: &[CoreInst]) -> HashSet<usize> {
    let mut entries = HashSet::new();
    for inst in instructions {
        match inst {
            CoreInst::Jump(target, _) | CoreInst::JumpIfFalse(target, _) => {
                if let Ok(index) = usize::try_from(*target) {
                    entries.insert(index);
                }
            }
            CoreInst::PushHandler(target, _) | CoreInst::JumpIfSetLocal { target, .. } => {
                if let Ok(index) = usize::try_from(*target) {
                    entries.insert(index);
                }
            }
            _ => {}
        }
    }
    entries
}

/// Returns `true` for an instruction after which control never continues.
fn is_unconditional_exit(inst: &CoreInst) -> bool {
    matches!(
        inst,
        CoreInst::Return { .. } | CoreInst::Jump(_, _) | CoreInst::Fail(_)
    )
}

/// Re-notes every absolute jump target through the index map.
///
/// A target whose instruction survived maps to its new index. A target whose
/// instruction was removed lands on the next surviving instruction, which is the
/// position control would actually reach; a target past the end becomes the end
/// of the stream.
fn remap_jump_targets(instructions: Vec<CoreInst>, map: &IndexMap) -> Vec<CoreInst> {
    let stream_len = instructions.len() as isize;
    let resolve = |target: isize| -> isize {
        if target < 0 {
            return target;
        }
        let old = target as usize;
        if let Some(Some(new)) = map.get(old) {
            return *new as isize;
        }
        // The instruction was removed: the next surviving one owns the address.
        map.iter()
            .skip(old + 1)
            .find_map(|slot| *slot)
            .map_or(stream_len, |new: usize| new as isize)
    };

    instructions
        .into_iter()
        .map(|inst| match inst {
            CoreInst::Jump(target, span) => CoreInst::Jump(resolve(target), span),
            CoreInst::JumpIfFalse(target, span) => CoreInst::JumpIfFalse(resolve(target), span),
            CoreInst::PushHandler(target, span) => CoreInst::PushHandler(resolve(target), span),
            CoreInst::JumpIfSetLocal { slot, target, span } => CoreInst::JumpIfSetLocal {
                slot,
                target: resolve(target),
                span,
            },
            other => other,
        })
        .collect()
}
