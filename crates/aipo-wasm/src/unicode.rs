//! Unicode-aware string helpers emitted into every module.
//!
//! The reference VM calls `char::to_uppercase`, `char::to_lowercase`, `str::trim`
//! and `unicode-normalization`; a compiled module cannot call Rust, so `build.rs`
//! turns those same sources into tables and this module copies them into linear
//! memory and emits the lookups that read them.
//!
//! The helpers live here rather than in `runtime.rs` because they share one data
//! layout and one set of code-point primitives. Every helper works on code points,
//! never on bytes: a `String` is UTF-8, and an index the program sees (`len`,
//! `find`, `slice`) counts characters, not bytes.
//!
//! Two invariants keep the emitted code reviewable. A branch condition is always
//! materialised into a local before the `if`, so the condition is written once and
//! the body reads it back — the encoder's stack machine makes a duplicated
//! condition silently push an extra value. And every helper writes only to the
//! shared scratch area (`SCRATCH`), so no helper has to allocate to return more
//! than one code point.

use crate::emitter::WasmEmitter;
use crate::types::{WasmFnType, WasmType};
use crate::unicode_tables as tables;
use std::collections::HashMap;
use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

/// Byte width of a `(u32, u32)` table row.
const PAIR_ROW: i32 = 8;
/// Byte width of an expansion row: `[key, len, out0, out1, out2]`.
const EXPANSION_ROW: i32 = 20;
/// Byte width of a `(u32, u32, u32)` table row.
const TRIPLE_ROW: i32 = 12;

/// Offset of the code-point scratch slot.
const SCRATCH_CP: i32 = 0;
/// Offset of the case-expansion scratch slots (three code points).
const SCRATCH_CASE: i32 = 4;
/// Bytes reserved for scratch.
const SCRATCH_LEN: i32 = 16;

/// Offsets of the tables copied into linear memory.
#[derive(Debug, Clone, Copy)]
pub(super) struct UnicodeTables {
    pub upper_one_to_one: i32,
    pub upper_one_to_one_count: i32,
    pub upper_expansions: i32,
    pub upper_expansions_count: i32,
    pub lower_one_to_one: i32,
    pub lower_one_to_one_count: i32,
    pub lower_expansions: i32,
    pub lower_expansions_count: i32,
    /// `(first, last)` inclusive ranges of Unicode whitespace.
    pub whitespace: i32,
    pub whitespace_count: i32,
    /// Sorted code points that have a canonical decomposition.
    pub decomposition_keys: i32,
    /// `count + 1` offsets into `decomposition_pool`.
    pub decomposition_offsets: i32,
    /// Concatenated full canonical decompositions.
    pub decomposition_pool: i32,
    pub decomposition_count: i32,
    /// `(first, last, class)` ranges with a non-zero combining class.
    pub combining_class: i32,
    pub combining_class_count: i32,
    /// `(first, second, composed)` primary composites, sorted by `(first, second)`.
    pub composition: i32,
    pub composition_count: i32,
    /// Sixteen bytes of scratch for a decoded code point and a case expansion.
    ///
    /// None of these helpers recurses, so one static slot serves them all and no
    /// call has to allocate.
    pub scratch: i32,
}

/// Copies the Unicode tables into linear memory starting at `base`.
///
/// Returns the table offsets and the first byte after the last table, so the caller
/// can keep the heap above them.
pub(super) fn embed(emitter: &mut WasmEmitter, base: i32) -> (UnicodeTables, i32) {
    let mut offset = align8(base);

    let (upper_one_to_one, upper_one_to_one_count) =
        push_pairs(emitter, &mut offset, tables::UPPER_ONE_TO_ONE);
    let (upper_expansions, upper_expansions_count) =
        push_expansions(emitter, &mut offset, tables::UPPER_EXPANSIONS);
    let (lower_one_to_one, lower_one_to_one_count) =
        push_pairs(emitter, &mut offset, tables::LOWER_ONE_TO_ONE);
    let (lower_expansions, lower_expansions_count) =
        push_expansions(emitter, &mut offset, tables::LOWER_EXPANSIONS);
    let (whitespace, whitespace_count) = push_pairs(emitter, &mut offset, tables::WHITESPACE);

    // Decomposition is three parallel arrays: sorted keys, one offset per key plus
    // a sentinel, and the concatenated expansions.
    let decomposition_keys = offset;
    let mut bytes = Vec::with_capacity(tables::DECOMPOSITION.len() * 4);
    for (key, _) in tables::DECOMPOSITION {
        bytes.extend_from_slice(&key.to_le_bytes());
    }
    offset += bytes.len() as i32;
    emitter.add_data_segment(0, decomposition_keys, bytes);

    let decomposition_offsets = offset;
    let mut bytes = Vec::with_capacity((tables::DECOMPOSITION.len() + 1) * 4);
    let mut pool_len = 0u32;
    for (_, values) in tables::DECOMPOSITION {
        bytes.extend_from_slice(&pool_len.to_le_bytes());
        pool_len += values.len() as u32;
    }
    bytes.extend_from_slice(&pool_len.to_le_bytes());
    offset += bytes.len() as i32;
    emitter.add_data_segment(0, decomposition_offsets, bytes);

    let decomposition_pool = offset;
    let mut bytes = Vec::with_capacity(pool_len as usize * 4);
    for (_, values) in tables::DECOMPOSITION {
        for value in *values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    offset += bytes.len() as i32;
    emitter.add_data_segment(0, decomposition_pool, bytes);
    let decomposition_count = tables::DECOMPOSITION.len() as i32;

    let combining_class = offset;
    let mut bytes = Vec::with_capacity(tables::COMBINING_CLASS.len() * TRIPLE_ROW as usize);
    for (first, last, class) in tables::COMBINING_CLASS {
        bytes.extend_from_slice(&first.to_le_bytes());
        bytes.extend_from_slice(&last.to_le_bytes());
        bytes.extend_from_slice(&u32::from(*class).to_le_bytes());
    }
    offset += bytes.len() as i32;
    emitter.add_data_segment(0, combining_class, bytes);
    let combining_class_count = tables::COMBINING_CLASS.len() as i32;

    let composition = offset;
    let mut bytes = Vec::with_capacity(tables::COMPOSITION.len() * TRIPLE_ROW as usize);
    for (first, second, composed) in tables::COMPOSITION {
        bytes.extend_from_slice(&first.to_le_bytes());
        bytes.extend_from_slice(&second.to_le_bytes());
        bytes.extend_from_slice(&composed.to_le_bytes());
    }
    offset += bytes.len() as i32;
    emitter.add_data_segment(0, composition, bytes);
    let composition_count = tables::COMPOSITION.len() as i32;

    let scratch = offset;
    offset += SCRATCH_LEN;
    emitter.add_data_segment(0, scratch, vec![0; SCRATCH_LEN as usize]);

    (
        UnicodeTables {
            upper_one_to_one,
            upper_one_to_one_count,
            upper_expansions,
            upper_expansions_count,
            lower_one_to_one,
            lower_one_to_one_count,
            lower_expansions,
            lower_expansions_count,
            whitespace,
            whitespace_count,
            decomposition_keys,
            decomposition_offsets,
            decomposition_pool,
            decomposition_count,
            combining_class,
            combining_class_count,
            composition,
            composition_count,
            scratch,
        },
        offset,
    )
}

fn push_pairs(emitter: &mut WasmEmitter, offset: &mut i32, rows: &[(u32, u32)]) -> (i32, i32) {
    let start = *offset;
    let mut bytes = Vec::with_capacity(rows.len() * PAIR_ROW as usize);
    for (key, value) in rows {
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    *offset += bytes.len() as i32;
    emitter.add_data_segment(0, start, bytes);
    (start, rows.len() as i32)
}

fn push_expansions(
    emitter: &mut WasmEmitter,
    offset: &mut i32,
    rows: &[(u32, &[u32])],
) -> (i32, i32) {
    let start = *offset;
    let mut bytes = Vec::with_capacity(rows.len() * EXPANSION_ROW as usize);
    for (key, values) in rows {
        debug_assert!(values.len() <= 3, "expansion longer than three slots");
        bytes.extend_from_slice(&key.to_le_bytes());
        bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for slot in 0..3 {
            bytes.extend_from_slice(&values.get(slot).copied().unwrap_or(0).to_le_bytes());
        }
    }
    *offset += bytes.len() as i32;
    emitter.add_data_segment(0, start, bytes);
    (start, rows.len() as i32)
}

fn align8(value: i32) -> i32 {
    (value + 7) & !7
}

/// Adds the Unicode helpers and returns the normalizer's index.
///
/// Only the normalizer is called by index, because concatenation and reversal
/// are emitted before the method dispatcher that resolves helpers by name.
pub(super) fn register(
    emitter: &mut WasmEmitter,
    tables: &UnicodeTables,
    alloc_func_idx: u32,
    functions: &mut HashMap<String, (u32, u32, WasmFnType)>,
) -> u32 {
    let utf8_len = add(
        emitter,
        &[WasmType::I32],
        &[WasmType::I32],
        build_utf8_len(),
    );
    let decode = add(
        emitter,
        &[WasmType::I32; 4],
        &[WasmType::I32],
        build_decode(),
    );
    let encode = add(
        emitter,
        &[WasmType::I32; 3],
        &[WasmType::I32],
        build_encode(),
    );
    let ccc = add(
        emitter,
        &[WasmType::I32],
        &[WasmType::I32],
        build_combining_class(tables),
    );
    let decomp_index = add(
        emitter,
        &[WasmType::I32],
        &[WasmType::I32],
        build_decomposition_index(tables),
    );
    let compose = add(
        emitter,
        &[WasmType::I32; 2],
        &[WasmType::I32],
        build_compose(tables),
    );
    let nfc = add(
        emitter,
        &[WasmType::I32],
        &[WasmType::I32],
        build_nfc(NfcContext {
            tables,
            alloc: alloc_func_idx,
            decode,
            encode,
            ccc,
            decomp_index,
            compose,
        }),
    );
    let case_full = add(
        emitter,
        &[WasmType::I32; 3],
        &[WasmType::I32],
        build_case_full(tables),
    );
    let case = add(
        emitter,
        &[WasmType::I32; 2],
        &[WasmType::I32],
        build_str_case(alloc_func_idx, utf8_len, decode, encode, case_full, nfc),
    );
    let is_space = add(
        emitter,
        &[WasmType::I32],
        &[WasmType::I32],
        build_is_space(tables),
    );
    let trim = add(
        emitter,
        &[WasmType::I32],
        &[WasmType::I32],
        build_str_trim(alloc_func_idx, decode, is_space),
    );

    emitter.export_function("__aipo_nfc", nfc);
    emitter.export_function("__aipo_str_case", case);
    emitter.export_function("__aipo_str_trim", trim);

    // The code generator resolves helpers by name, so the ones it calls directly
    // join the same table as the rest of the runtime.
    for (name, index, params, results) in [
        ("__aipo_nfc", nfc, vec![WasmType::I32], vec![WasmType::I32]),
        (
            "__aipo_str_case",
            case,
            vec![WasmType::I32, WasmType::I32],
            vec![WasmType::I32],
        ),
        (
            "__aipo_str_trim",
            trim,
            vec![WasmType::I32],
            vec![WasmType::I32],
        ),
    ] {
        let fn_type = WasmFnType::new(params, results);
        let type_idx = emitter.add_type(fn_type.clone());
        functions.insert(name.to_string(), (index, type_idx, fn_type));
    }

    nfc
}

fn add(
    emitter: &mut WasmEmitter,
    params: &[WasmType],
    results: &[WasmType],
    func: Function,
) -> u32 {
    let type_idx = emitter.add_type(WasmFnType::new(params.to_vec(), results.to_vec()));
    emitter.add_function(type_idx, func)
}

// ---------------------------------------------------------------------------
// Instruction builder
// ---------------------------------------------------------------------------

/// Minimal instruction builder.
///
/// These helpers are long sequences of stack arithmetic; writing
/// `func.instruction(&Instruction::…);` for each of them hides the shape of the
/// algorithm. This wrapper keeps the emitted order explicit while making the
/// common operations one line each.
///
/// It deliberately has no closure-based `if`/`while`: a Wasm `if` takes its
/// condition from the stack, so a helper that re-evaluates the condition inside
/// the body pushes a second value and corrupts the frame. Conditions are stored in
/// a local and read back instead.
struct A<'a> {
    func: &'a mut Function,
}

impl<'a> A<'a> {
    fn new(func: &'a mut Function) -> Self {
        Self { func }
    }

    fn i(&mut self, instruction: Instruction) -> &mut Self {
        self.func.instruction(&instruction);
        self
    }

    fn c(&mut self, value: i32) -> &mut Self {
        self.i(Instruction::I32Const(value))
    }

    fn g(&mut self, local: u32) -> &mut Self {
        self.i(Instruction::LocalGet(local))
    }

    fn s(&mut self, local: u32) -> &mut Self {
        self.i(Instruction::LocalSet(local))
    }

    fn add(&mut self) -> &mut Self {
        self.i(Instruction::I32Add)
    }

    fn sub(&mut self) -> &mut Self {
        self.i(Instruction::I32Sub)
    }

    fn mul(&mut self) -> &mut Self {
        self.i(Instruction::I32Mul)
    }

    fn div_u(&mut self) -> &mut Self {
        self.i(Instruction::I32DivU)
    }

    fn rem_u(&mut self) -> &mut Self {
        self.i(Instruction::I32RemU)
    }

    fn and(&mut self) -> &mut Self {
        self.i(Instruction::I32And)
    }

    fn or(&mut self) -> &mut Self {
        self.i(Instruction::I32Or)
    }

    fn shl(&mut self) -> &mut Self {
        self.i(Instruction::I32Shl)
    }

    fn shr_u(&mut self) -> &mut Self {
        self.i(Instruction::I32ShrU)
    }

    fn eqz(&mut self) -> &mut Self {
        self.i(Instruction::I32Eqz)
    }

    fn eq(&mut self) -> &mut Self {
        self.i(Instruction::I32Eq)
    }

    fn lt_u(&mut self) -> &mut Self {
        self.i(Instruction::I32LtU)
    }

    fn le_u(&mut self) -> &mut Self {
        self.i(Instruction::I32LeU)
    }

    fn ge_u(&mut self) -> &mut Self {
        self.i(Instruction::I32GeU)
    }

    fn ge_s(&mut self) -> &mut Self {
        self.i(Instruction::I32GeS)
    }

    fn gt_s(&mut self) -> &mut Self {
        self.i(Instruction::I32GtS)
    }

    fn lt_s(&mut self) -> &mut Self {
        self.i(Instruction::I32LtS)
    }

    fn select(&mut self) -> &mut Self {
        self.i(Instruction::Select)
    }

    fn call(&mut self, index: u32) -> &mut Self {
        self.i(Instruction::Call(index))
    }

    fn ret(&mut self) -> &mut Self {
        self.i(Instruction::Return)
    }

    fn block(&mut self) -> &mut Self {
        self.i(Instruction::Block(BlockType::Empty))
    }

    fn loop_(&mut self) -> &mut Self {
        self.i(Instruction::Loop(BlockType::Empty))
    }

    fn if_(&mut self) -> &mut Self {
        self.i(Instruction::If(BlockType::Empty))
    }

    fn else_(&mut self) -> &mut Self {
        self.i(Instruction::Else)
    }

    fn end(&mut self) -> &mut Self {
        self.i(Instruction::End)
    }

    fn br(&mut self, depth: u32) -> &mut Self {
        self.i(Instruction::Br(depth))
    }

    fn br_if(&mut self, depth: u32) -> &mut Self {
        self.i(Instruction::BrIf(depth))
    }

    fn load(&mut self, offset: u32) -> &mut Self {
        self.i(Instruction::I32Load(MemArg {
            offset: u64::from(offset),
            align: 2,
            memory_index: 0,
        }))
    }

    fn store(&mut self, offset: u32) -> &mut Self {
        self.i(Instruction::I32Store(MemArg {
            offset: u64::from(offset),
            align: 2,
            memory_index: 0,
        }))
    }

    fn load8(&mut self, offset: u32) -> &mut Self {
        self.i(Instruction::I32Load8U(MemArg {
            offset: u64::from(offset),
            align: 0,
            memory_index: 0,
        }))
    }

    fn store8(&mut self, offset: u32) -> &mut Self {
        self.i(Instruction::I32Store8(MemArg {
            offset: u64::from(offset),
            align: 0,
            memory_index: 0,
        }))
    }

    /// Pushes `base + index * stride`.
    fn addr(&mut self, base: i32, index: u32, stride: i32) -> &mut Self {
        self.c(base).g(index).c(stride).mul().add()
    }

    /// Pushes the `u32` at `base + index * stride`.
    fn elem(&mut self, base: i32, index: u32, stride: i32) -> &mut Self {
        self.addr(base, index, stride).load(0)
    }

    /// Pushes the string byte length at `ptr`.
    fn str_len(&mut self, ptr: u32) -> &mut Self {
        self.g(ptr).load(0)
    }
}

// ---------------------------------------------------------------------------
// Code-point primitives
// ---------------------------------------------------------------------------

/// `__aipo_utf8_len(cp: i32) -> i32`
fn build_utf8_len() -> Function {
    let mut func = Function::new([]);
    let mut a = A::new(&mut func);

    a.g(0).c(0x80).lt_u();
    a.if_();
    a.c(1).ret();
    a.end();

    a.g(0).c(0x800).lt_u();
    a.if_();
    a.c(2).ret();
    a.end();

    a.g(0).c(0x1_0000).lt_u();
    a.if_();
    a.c(3).ret();
    a.end();

    a.c(4);
    a.end();
    func
}

/// `__aipo_decode(ptr: i32, pos: i32, len: i32, out: i32) -> i32`
///
/// Decodes the UTF-8 sequence starting at byte `pos`, stores the code point at
/// `out` and returns the byte width consumed. The width is clamped to what is left
/// before any continuation byte is read, so a truncated tail cannot read past the
/// string and decodes as its lead byte.
fn build_decode() -> Function {
    // 4=addr, 5=b0, 6=cp, 7=width
    let mut func = Function::new([(4, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.g(0).g(1).add().c(4).add().s(4);
    a.g(4).load8(0).s(5);

    a.c(1).s(7);
    a.g(5).c(0x80).lt_u();
    a.if_();
    a.else_();
    a.c(2).s(7);
    a.g(5).c(0xE0).lt_u();
    a.if_();
    a.else_();
    a.c(3).s(7);
    a.g(5).c(0xF0).lt_u();
    a.if_();
    a.else_();
    a.c(4).s(7);
    a.end();
    a.end();
    a.end();

    // A sequence that runs past `len` is a truncated tail: keep the lead byte.
    a.g(1).g(7).add().g(2).gt_s();
    a.if_();
    a.c(1).s(7);
    a.end();

    a.g(7).c(1).eq();
    a.if_();
    a.g(5).s(6);
    a.else_();
    a.g(7).c(2).eq();
    a.if_();
    a.g(5).c(0x1F).and().c(6).shl();
    a.g(4).load8(1).c(0x3F).and().or().s(6);
    a.else_();
    a.g(7).c(3).eq();
    a.if_();
    a.g(5).c(0x0F).and().c(12).shl();
    a.g(4).load8(1).c(0x3F).and().c(6).shl().or();
    a.g(4).load8(2).c(0x3F).and().or().s(6);
    a.else_();
    a.g(5).c(0x07).and().c(18).shl();
    a.g(4).load8(1).c(0x3F).and().c(12).shl().or();
    a.g(4).load8(2).c(0x3F).and().c(6).shl().or();
    a.g(4).load8(3).c(0x3F).and().or().s(6);
    a.end();
    a.end();
    a.end();

    a.g(3).g(6).store(0);
    a.g(7);
    a.end();
    func
}

/// `__aipo_encode(ptr: i32, pos: i32, cp: i32) -> i32`
///
/// Writes `cp` as UTF-8 at byte `pos` and returns the width written.
fn build_encode() -> Function {
    // 3=addr
    let mut func = Function::new([(1, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.g(0).g(1).add().c(4).add().s(3);

    a.g(2).c(0x80).lt_u();
    a.if_();
    a.g(3).g(2).store8(0);
    a.c(1).ret();
    a.end();

    a.g(2).c(0x800).lt_u();
    a.if_();
    a.g(3).g(2).c(6).shr_u().c(0xC0).or().store8(0);
    a.g(3).g(2).c(0x3F).and().c(0x80).or().store8(1);
    a.c(2).ret();
    a.end();

    a.g(2).c(0x1_0000).lt_u();
    a.if_();
    a.g(3).g(2).c(12).shr_u().c(0xE0).or().store8(0);
    a.g(3)
        .g(2)
        .c(6)
        .shr_u()
        .c(0x3F)
        .and()
        .c(0x80)
        .or()
        .store8(1);
    a.g(3).g(2).c(0x3F).and().c(0x80).or().store8(2);
    a.c(3).ret();
    a.end();

    a.g(3).g(2).c(18).shr_u().c(0xF0).or().store8(0);
    a.g(3)
        .g(2)
        .c(12)
        .shr_u()
        .c(0x3F)
        .and()
        .c(0x80)
        .or()
        .store8(1);
    a.g(3)
        .g(2)
        .c(6)
        .shr_u()
        .c(0x3F)
        .and()
        .c(0x80)
        .or()
        .store8(2);
    a.g(3).g(2).c(0x3F).and().c(0x80).or().store8(3);
    a.c(4);
    a.end();
    func
}

/// `__aipo_ccc(cp: i32) -> i32`
///
/// Canonical combining class, from the `(first, last, class)` range table.
fn build_combining_class(t: &UnicodeTables) -> Function {
    // 1=lo, 2=hi, 3=mid
    let mut func = Function::new([(3, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.c(0).s(1);
    a.c(t.combining_class_count).s(2);
    a.block();
    a.loop_();
    a.g(1).g(2).ge_s().br_if(1);
    a.g(1).g(2).add().c(1).shr_u().s(3);

    a.elem(t.combining_class, 3, TRIPLE_ROW).g(0).le_u();
    a.if_();
    a.addr(t.combining_class, 3, TRIPLE_ROW).load(4).g(0).ge_u();
    a.if_();
    a.addr(t.combining_class, 3, TRIPLE_ROW).load(8).ret();
    a.else_();
    a.g(3).c(1).add().s(1);
    a.end();
    a.else_();
    a.g(3).s(2);
    a.end();

    a.br(0);
    a.end();
    a.end();

    a.c(0);
    a.end();
    func
}

/// `__aipo_decomp_index(cp: i32) -> i32`
///
/// Index of `cp` in the sorted decomposition keys, or `-1`.
fn build_decomposition_index(t: &UnicodeTables) -> Function {
    // 1=lo, 2=hi, 3=mid
    let mut func = Function::new([(3, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.c(0).s(1);
    a.c(t.decomposition_count).s(2);
    a.block();
    a.loop_();
    a.g(1).g(2).ge_s().br_if(1);
    a.g(1).g(2).add().c(1).shr_u().s(3);
    a.elem(t.decomposition_keys, 3, 4).g(0).lt_u();
    a.if_();
    a.g(3).c(1).add().s(1);
    a.else_();
    a.g(3).s(2);
    a.end();
    a.br(0);
    a.end();
    a.end();

    // `lo == hi`; a hit is the one key equal to `cp`.
    a.g(1).c(t.decomposition_count).lt_u();
    a.if_();
    a.elem(t.decomposition_keys, 1, 4).g(0).eq();
    a.if_();
    a.g(1).ret();
    a.end();
    a.end();

    a.c(-1);
    a.end();
    func
}

/// `__aipo_compose(a: i32, b: i32) -> i32`
///
/// The canonical composite of `a` and `b`, or `0` when there is none. Hangul is
/// algorithmic and is handled before the table.
fn build_compose(t: &UnicodeTables) -> Function {
    // 2=lo, 3=hi, 4=mid
    let mut func = Function::new([(3, ValType::I32)]);
    let mut a = A::new(&mut func);

    // Hangul L + V -> LV.
    a.g(0).c(0x1100).ge_u();
    a.g(0).c(0x1112).le_u();
    a.and();
    a.g(1).c(0x1161).ge_u();
    a.and();
    a.g(1).c(0x1175).le_u();
    a.and();
    a.if_();
    a.c(0xAC00)
        .g(0)
        .c(0x1100)
        .sub()
        .c(588)
        .mul()
        .add()
        .g(1)
        .c(0x1161)
        .sub()
        .c(28)
        .mul()
        .add()
        .ret();
    a.end();

    // Hangul LV + T -> LVT.
    a.g(0).c(0xAC00).ge_u();
    a.g(0).c(0xD7A3).le_u();
    a.and();
    a.g(0).c(0xAC00).sub().c(28).rem_u().eqz();
    a.and();
    a.g(1).c(0x11A8).ge_u();
    a.and();
    a.g(1).c(0x11C2).le_u();
    a.and();
    a.if_();
    a.g(0).g(1).add().c(0x11A7).sub().ret();
    a.end();

    a.c(0).s(2);
    a.c(t.composition_count).s(3);
    a.block();
    a.loop_();
    a.g(2).g(3).ge_s().br_if(1);
    a.g(2).g(3).add().c(1).shr_u().s(4);

    // Order the row's key `(first, second)` against `(a, b)`.
    a.elem(t.composition, 4, TRIPLE_ROW).g(0).lt_u();
    a.if_();
    a.g(4).c(1).add().s(2);
    a.else_();
    a.elem(t.composition, 4, TRIPLE_ROW).g(0).eq();
    a.if_();
    a.addr(t.composition, 4, TRIPLE_ROW).load(4).g(1).lt_u();
    a.if_();
    a.g(4).c(1).add().s(2);
    a.else_();
    a.g(4).s(3);
    a.end();
    a.else_();
    a.g(4).s(3);
    a.end();
    a.end();

    a.br(0);
    a.end();
    a.end();

    a.g(2).c(t.composition_count).lt_u();
    a.if_();
    a.elem(t.composition, 2, TRIPLE_ROW).g(0).eq();
    a.addr(t.composition, 2, TRIPLE_ROW).load(4).g(1).eq();
    a.and();
    a.if_();
    a.addr(t.composition, 2, TRIPLE_ROW).load(8).ret();
    a.end();
    a.end();

    a.c(0);
    a.end();
    func
}

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

/// Shared context for the normalizer.
#[derive(Clone, Copy)]
struct NfcContext<'a> {
    tables: &'a UnicodeTables,
    alloc: u32,
    decode: u32,
    encode: u32,
    ccc: u32,
    decomp_index: u32,
    compose: u32,
}

/// Locals shared by the two expansion passes.
#[derive(Clone, Copy)]
struct Expand {
    cp: u32,
    count: u32,
    buf: u32,
    m: u32,
    tmp: u32,
    start: u32,
    end: u32,
    k: u32,
    changed: u32,
    store: bool,
}

/// Emits one code point of an expansion: either counted or appended to `buf`.
///
/// The value is on the stack on entry.
fn emit_one(a: &mut A<'_>, e: &Expand) {
    if e.store {
        a.s(e.tmp);
        a.g(e.buf).g(e.m).c(4).mul().add().g(e.tmp).store(0);
        a.g(e.m).c(1).add().s(e.m);
    } else {
        a.i(Instruction::Drop);
        a.g(e.count).c(1).add().s(e.count);
    }
}

/// Emits the full canonical decomposition of `e.cp` (Hangul included).
fn emit_expand(a: &mut A<'_>, t: &UnicodeTables, e: &Expand, decomp_index: u32) {
    a.g(e.cp).c(0xAC00).ge_u();
    a.g(e.cp).c(0xD7A3).le_u();
    a.and();
    a.if_();

    a.c(1).s(e.changed);
    if e.store {
        // L = 0x1100 + (cp - 0xAC00) / 588
        a.c(0x1100).g(e.cp).c(0xAC00).sub().c(588).div_u().add();
        emit_one(a, e);
        // V = 0x1161 + ((cp - 0xAC00) % 588) / 28
        a.c(0x1161)
            .g(e.cp)
            .c(0xAC00)
            .sub()
            .c(588)
            .rem_u()
            .c(28)
            .div_u()
            .add();
        emit_one(a, e);
        // T = 0x11A7 + (cp - 0xAC00) % 28, only when the syllable has one.
        a.g(e.cp).c(0xAC00).sub().c(28).rem_u().s(e.tmp);
        a.g(e.tmp).eqz();
        a.if_();
        a.else_();
        a.c(0x11A7).g(e.tmp).add();
        emit_one(a, e);
        a.end();
    } else {
        a.g(e.count).c(2).add().s(e.count);
        a.g(e.cp).c(0xAC00).sub().c(28).rem_u().eqz();
        a.if_();
        a.else_();
        a.g(e.count).c(1).add().s(e.count);
        a.end();
    }

    a.else_();
    a.g(e.cp).call(decomp_index).s(e.tmp);
    a.g(e.tmp).c(0).ge_s();
    a.if_();
    a.c(1).s(e.changed);
    a.c(t.decomposition_offsets)
        .g(e.tmp)
        .c(4)
        .mul()
        .add()
        .load(0)
        .s(e.start);
    a.c(t.decomposition_offsets)
        .g(e.tmp)
        .c(4)
        .mul()
        .add()
        .load(4)
        .s(e.end);
    a.g(e.start).s(e.k);
    a.block();
    a.loop_();
    a.g(e.k).g(e.end).ge_s().br_if(1);
    a.elem(t.decomposition_pool, e.k, 4);
    emit_one(a, e);
    a.g(e.k).c(1).add().s(e.k);
    a.br(0);
    a.end();
    a.end();
    a.else_();
    a.g(e.cp);
    emit_one(a, e);
    a.end();
    a.end();
}

/// `__aipo_nfc(ptr: i32) -> i32`
///
/// Returns a string in NFC. Pure ASCII is returned untouched, and a non-ASCII
/// string whose decomposition, ordering and recomposition leave every code point
/// where it was is returned untouched too, so the common cases copy nothing.
fn build_nfc(ctx: NfcContext<'_>) -> Function {
    let t = ctx.tables;
    // 1=len, 2=pos, 3=w, 4=n, 5=buf, 6=m, 7=i, 8=cc, 9=j, 10=prev,
    // 11=starter, 12=last_cc, 13=wpos, 14=r, 15=cp, 16=composed, 17=idx,
    // 18=start, 19=end, 20=k, 21=out, 22=cur, 23=changed, 24=tmp
    let mut func = Function::new([(24, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.str_len(0).s(1);

    // ASCII needs no normalization at all: scan the bytes and stop at the first
    // one that is not 7-bit.
    a.c(0).s(2);
    a.block();
    a.loop_();
    a.g(2).g(1).ge_s().br_if(1);
    a.g(0).g(2).add().load8(4).c(0x80).lt_u().eqz().br_if(1);
    a.g(2).c(1).add().s(2);
    a.br(0);
    a.end();
    a.end();
    a.g(2).g(1).ge_s();
    a.if_();
    a.g(0).ret();
    a.end();

    // Pass 1: the fully decomposed code-point count.
    a.c(0).s(4);
    a.c(0).s(23);
    a.c(0).s(2);
    a.block();
    a.loop_();
    a.g(2).g(1).ge_s().br_if(1);
    a.g(0).g(2).g(1).c(t.scratch).call(ctx.decode).s(3);
    a.c(t.scratch).load(0).s(15);
    a.g(15).c(0xAC00).ge_u();
    a.g(15).c(0xD7A3).le_u();
    a.and();
    a.if_();
    a.g(4).c(2).add().s(4);
    a.g(15).c(0xAC00).sub().c(28).rem_u().eqz();
    a.if_();
    a.else_();
    a.g(4).c(1).add().s(4);
    a.end();
    a.c(1).s(23);
    a.else_();
    a.g(15).call(ctx.decomp_index).s(17);
    a.g(17).c(0).ge_s();
    a.if_();
    a.c(1).s(23);
    a.c(t.decomposition_offsets).g(17).c(4).mul().add().load(4);
    a.c(t.decomposition_offsets).g(17).c(4).mul().add().load(0);
    a.sub().g(4).add().s(4);
    a.else_();
    a.g(4).c(1).add().s(4);
    a.end();
    a.end();
    a.g(2).g(3).add().s(2);
    a.br(0);
    a.end();
    a.end();

    // Pass 2: write the decomposition into a scratch code-point array.
    a.g(4).c(4).mul().call(ctx.alloc).s(5);
    a.c(0).s(6);
    a.c(0).s(2);
    a.block();
    a.loop_();
    a.g(2).g(1).ge_s().br_if(1);
    a.g(0).g(2).g(1).c(t.scratch).call(ctx.decode).s(3);
    a.c(t.scratch).load(0).s(15);
    emit_expand(
        &mut a,
        t,
        &Expand {
            cp: 15,
            count: 4,
            buf: 5,
            m: 6,
            tmp: 24,
            start: 18,
            end: 19,
            k: 20,
            changed: 23,
            store: true,
        },
        ctx.decomp_index,
    );
    a.g(2).g(3).add().s(2);
    a.br(0);
    a.end();
    a.end();

    // Canonical ordering: a stable insertion sort by combining class, bounded by
    // starters, which have class 0 and never move.
    a.c(1).s(7);
    a.block();
    a.loop_();
    a.g(7).g(6).ge_s().br_if(1);
    a.g(5).g(7).c(4).mul().add().load(0).call(ctx.ccc).s(8);
    a.g(8).eqz();
    a.if_();
    a.else_();
    a.g(7).s(9);
    a.block();
    a.loop_();
    a.g(9).eqz().br_if(1);
    a.g(5)
        .g(9)
        .c(1)
        .sub()
        .c(4)
        .mul()
        .add()
        .load(0)
        .call(ctx.ccc)
        .s(10);
    a.g(10).eqz().br_if(1);
    a.g(10).g(8).le_u().br_if(1);
    // swap buf[j-1] and buf[j]
    a.g(5).g(9).c(1).sub().c(4).mul().add().load(0).s(24);
    a.g(5).g(9).c(1).sub().c(4).mul().add();
    a.g(5).g(9).c(4).mul().add().load(0);
    a.store(0);
    a.g(5).g(9).c(4).mul().add().g(24);
    a.store(0);
    a.c(1).s(23);
    a.g(9).c(1).sub().s(9);
    a.br(0);
    a.end();
    a.end();
    a.end();
    a.g(7).c(1).add().s(7);
    a.br(0);
    a.end();
    a.end();

    // Canonical composition, in place: the write cursor never passes the read one.
    a.c(-1).s(11);
    a.c(0).s(12);
    a.c(0).s(13);
    a.c(0).s(14);
    a.block();
    a.loop_();
    a.g(14).g(6).ge_s().br_if(1);
    a.g(5).g(14).c(4).mul().add().load(0).s(15);
    a.g(15).call(ctx.ccc).s(8);
    a.c(0).s(16);

    // A starter absorbs the next character only when nothing in between blocks it:
    // `last_cc == 0` (nothing between) or `last_cc < cc`.
    a.g(11).c(0).ge_s();
    a.if_();
    a.g(12).eqz();
    a.g(12).g(8).lt_u();
    a.or();
    a.if_();
    a.g(5)
        .g(11)
        .c(4)
        .mul()
        .add()
        .load(0)
        .g(15)
        .call(ctx.compose)
        .s(16);
    a.end();
    a.end();

    a.g(16).eqz();
    a.if_();
    a.g(8).eqz();
    a.if_();
    a.g(13).s(11);
    a.end();
    a.g(8).s(12);
    a.g(5).g(13).c(4).mul().add().g(15);
    a.store(0);
    a.g(13).c(1).add().s(13);
    a.else_();
    a.g(5).g(11).c(4).mul().add().g(16);
    a.store(0);
    a.c(1).s(23);
    a.end();

    a.g(14).c(1).add().s(14);
    a.br(0);
    a.end();
    a.end();

    a.g(23).eqz();
    a.if_();
    a.g(0).ret();
    a.end();

    // Encode the composed code points into a fresh string.
    a.g(13).c(4).mul().c(4).add().call(ctx.alloc).s(21);
    a.c(0).s(22);
    a.c(0).s(20);
    a.block();
    a.loop_();
    a.g(20).g(13).ge_s().br_if(1);
    a.g(21)
        .g(22)
        .g(5)
        .g(20)
        .c(4)
        .mul()
        .add()
        .load(0)
        .call(ctx.encode)
        .g(22)
        .add()
        .s(22);
    a.g(20).c(1).add().s(20);
    a.br(0);
    a.end();
    a.end();
    a.g(21).g(22).store(0);
    a.g(21);
    a.end();
    func
}

// ---------------------------------------------------------------------------
// Case mapping
// ---------------------------------------------------------------------------

/// `__aipo_case_full(cp: i32, upper: i32, out: i32) -> i32`
///
/// Writes the 1..3 code points `cp` maps to and returns how many.
fn build_case_full(t: &UnicodeTables) -> Function {
    // 3=base, 4=count, 5=lo, 6=hi, 7=mid, 8=n, 9=k, 10=row
    let mut func = Function::new([(8, ValType::I32)]);
    let mut a = A::new(&mut func);

    // The one-to-one mapping first; the multi-code-point expansions are rarer.
    for (upper, lower, upper_count, lower_count, stride) in [
        (
            t.upper_one_to_one,
            t.lower_one_to_one,
            t.upper_one_to_one_count,
            t.lower_one_to_one_count,
            PAIR_ROW,
        ),
        (
            t.upper_expansions,
            t.lower_expansions,
            t.upper_expansions_count,
            t.lower_expansions_count,
            EXPANSION_ROW,
        ),
    ] {
        a.c(upper).c(lower).g(1).select().s(3);
        a.c(upper_count).c(lower_count).g(1).select().s(4);
        lower_bound(&mut a, 3, 4, 5, 6, 7, 0, stride);

        a.g(5).g(4).lt_u();
        a.if_();
        a.g(3).g(5).c(stride).mul().add().load(0).g(0).eq();
        a.if_();
        a.g(3).g(5).c(stride).mul().add().s(10);
        if stride == PAIR_ROW {
            a.g(2).g(10).load(4).store(0);
            a.c(1).ret();
        } else {
            a.g(10).load(4).s(8);
            a.c(0).s(9);
            a.block();
            a.loop_();
            a.g(9).g(8).ge_s().br_if(1);
            a.g(2).g(9).c(4).mul().add();
            a.g(10).g(9).c(4).mul().add().load(8);
            a.store(0);
            a.g(9).c(1).add().s(9);
            a.br(0);
            a.end();
            a.end();
            a.g(8).ret();
        }
        a.end();
        a.end();
    }

    // No mapping: the character maps to itself.
    a.g(2).g(0).store(0);
    a.c(1);
    a.end();
    func
}

/// Emits a lower-bound binary search over a sorted key table.
///
/// Leaves `lo` on the first row whose key is not less than `cp`.
#[allow(clippy::too_many_arguments)]
fn lower_bound(
    a: &mut A<'_>,
    base: u32,
    count: u32,
    lo: u32,
    hi: u32,
    mid: u32,
    cp: u32,
    stride: i32,
) {
    a.c(0).s(lo);
    a.g(count).s(hi);
    a.block();
    a.loop_();
    a.g(lo).g(hi).ge_s().br_if(1);
    a.g(lo).g(hi).add().c(1).shr_u().s(mid);
    a.g(base).g(mid).c(stride).mul().add().load(0).g(cp).lt_u();
    a.if_();
    a.g(mid).c(1).add().s(lo);
    a.else_();
    a.g(mid).s(hi);
    a.end();
    a.br(0);
    a.end();
    a.end();
}

/// `__aipo_str_case(ptr: i32, upper: i32) -> i32`
fn build_str_case(
    alloc: u32,
    utf8_len: u32,
    decode: u32,
    encode: u32,
    case_full: u32,
    nfc: u32,
) -> Function {
    // 2=len, 3=pos, 4=w, 5=out_len, 6=out, 7=cur, 8=n, 9=k
    let mut func = Function::new([(8, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.str_len(0).s(2);

    // Pass 1: the exact byte length, because an expansion changes it.
    a.c(0).s(3);
    a.c(0).s(5);
    a.block();
    a.loop_();
    a.g(3).g(2).ge_s().br_if(1);
    a.g(0).g(3).g(2).c(SCRATCH_CP).call(decode).s(4);
    a.c(SCRATCH_CP)
        .load(0)
        .g(1)
        .c(SCRATCH_CASE)
        .call(case_full)
        .s(8);
    a.c(0).s(9);
    a.block();
    a.loop_();
    a.g(9).g(8).ge_s().br_if(1);
    a.g(5)
        .c(SCRATCH_CASE)
        .g(9)
        .c(4)
        .mul()
        .add()
        .load(0)
        .call(utf8_len)
        .g(5)
        .add()
        .s(5);
    a.g(9).c(1).add().s(9);
    a.br(0);
    a.end();
    a.end();
    a.g(3).g(4).add().s(3);
    a.br(0);
    a.end();
    a.end();

    a.g(5).c(4).add().call(alloc).s(6);

    // Pass 2: write the mapping.
    a.c(0).s(3);
    a.c(0).s(7);
    a.block();
    a.loop_();
    a.g(3).g(2).ge_s().br_if(1);
    a.g(0).g(3).g(2).c(SCRATCH_CP).call(decode).s(4);
    a.c(SCRATCH_CP)
        .load(0)
        .g(1)
        .c(SCRATCH_CASE)
        .call(case_full)
        .s(8);
    a.c(0).s(9);
    a.block();
    a.loop_();
    a.g(9).g(8).ge_s().br_if(1);
    a.g(6)
        .g(7)
        .c(SCRATCH_CASE)
        .g(9)
        .c(4)
        .mul()
        .add()
        .load(0)
        .call(encode)
        .g(7)
        .add()
        .s(7);
    a.g(9).c(1).add().s(9);
    a.br(0);
    a.end();
    a.end();
    a.g(3).g(4).add().s(3);
    a.br(0);
    a.end();
    a.end();
    a.g(6).g(7).store(0);

    // Case mapping can join a base with a mark (`İ` lowercases to `i` + U+0307),
    // so the result is re-normalized like the VM does.
    a.g(6).call(nfc);
    a.end();
    func
}

// ---------------------------------------------------------------------------
// Trimming
// ---------------------------------------------------------------------------

/// `__aipo_is_space(cp: i32) -> i32`
fn build_is_space(t: &UnicodeTables) -> Function {
    // 1=lo, 2=hi, 3=mid
    let mut func = Function::new([(3, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.c(0).s(1);
    a.c(t.whitespace_count).s(2);
    a.block();
    a.loop_();
    a.g(1).g(2).ge_s().br_if(1);
    a.g(1).g(2).add().c(1).shr_u().s(3);
    a.elem(t.whitespace, 3, PAIR_ROW).g(0).le_u();
    a.if_();
    a.addr(t.whitespace, 3, PAIR_ROW).load(4).g(0).ge_u();
    a.if_();
    a.c(1).ret();
    a.else_();
    a.g(3).c(1).add().s(1);
    a.end();
    a.else_();
    a.g(3).s(2);
    a.end();
    a.br(0);
    a.end();
    a.end();

    a.c(0);
    a.end();
    func
}

/// `__aipo_str_trim(ptr: i32) -> i32`
///
/// Drops leading and trailing Unicode whitespace. The result is a byte substring of
/// an already-NFC string, so it keeps the invariant without re-normalizing.
fn build_str_trim(alloc: u32, decode: u32, is_space: u32) -> Function {
    // 1=len, 2=pos, 3=w, 4=start, 5=end, 6=out, 7=i, 8=count
    let mut func = Function::new([(8, ValType::I32)]);
    let mut a = A::new(&mut func);

    a.str_len(0).s(1);
    a.c(0).s(2);
    a.c(-1).s(4);
    a.c(0).s(5);
    a.block();
    a.loop_();
    a.g(2).g(1).ge_s().br_if(1);
    a.g(0).g(2).g(1).c(SCRATCH_CP).call(decode).s(3);
    a.c(SCRATCH_CP).load(0).call(is_space).eqz();
    a.if_();
    a.g(4).c(0).lt_s();
    a.if_();
    a.g(2).s(4);
    a.end();
    a.g(2).g(3).add().s(5);
    a.end();
    a.g(2).g(3).add().s(2);
    a.br(0);
    a.end();
    a.end();

    // An all-whitespace string trims to the empty string.
    a.g(4).c(0).lt_s();
    a.if_();
    a.c(0).s(4);
    a.c(0).s(5);
    a.end();

    // Nothing to trim: the input is already the answer.
    a.g(4).eqz();
    a.g(5).g(1).eq();
    a.and();
    a.if_();
    a.g(0).ret();
    a.end();

    a.g(5).g(4).sub().s(8);
    a.g(8).c(4).add().call(alloc).s(6);
    a.c(0).s(7);
    a.block();
    a.loop_();
    a.g(7).g(8).ge_s().br_if(1);
    a.g(6).g(7).add();
    a.g(0).g(4).add().g(7).add().load8(4);
    a.store8(4);
    a.g(7).c(1).add().s(7);
    a.br(0);
    a.end();
    a.end();
    a.g(6).g(8).store(0);
    a.g(6);
    a.end();
    func
}
