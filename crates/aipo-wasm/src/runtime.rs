//! Runtime helper functions emitted into every module.
//!
//! These are hand-built Wasm functions and not host imports: strings, lists,
//! dictionaries, bytes, ranges and tasks are all implemented once, in the module
//! itself, so a compiled program is self-contained. Each helper obeys the same
//! memory layout the code generator assumes — see the layout note at the top of
//! `memory.rs`.

use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

pub(super) fn build_allocator_function() -> Function {
    let mut alloc_fn = Function::new([(2, ValType::I32)]);
    // local 0: size (param)
    // local 1: old_ptr
    // local 2: new_ptr

    // Align size to multiple of 8: size = (size + 7) & -8
    alloc_fn.instruction(&Instruction::LocalGet(0));
    alloc_fn.instruction(&Instruction::I32Const(7));
    alloc_fn.instruction(&Instruction::I32Add);
    alloc_fn.instruction(&Instruction::I32Const(-8));
    alloc_fn.instruction(&Instruction::I32And);
    alloc_fn.instruction(&Instruction::LocalSet(0));

    // old_ptr = global.get 0
    alloc_fn.instruction(&Instruction::GlobalGet(0));
    alloc_fn.instruction(&Instruction::LocalSet(1));

    // new_ptr = old_ptr + size
    alloc_fn.instruction(&Instruction::LocalGet(1));
    alloc_fn.instruction(&Instruction::LocalGet(0));
    alloc_fn.instruction(&Instruction::I32Add);
    alloc_fn.instruction(&Instruction::LocalSet(2));

    // Check if new_ptr > (memory.size << 16)
    // If so, grow memory dynamically
    alloc_fn.instruction(&Instruction::LocalGet(2));
    alloc_fn.instruction(&Instruction::MemorySize(0));
    alloc_fn.instruction(&Instruction::I32Const(16));
    alloc_fn.instruction(&Instruction::I32Shl);
    alloc_fn.instruction(&Instruction::I32GtU);
    alloc_fn.instruction(&Instruction::If(BlockType::Empty));

    // delta_pages = ((new_ptr - (memory.size << 16)) + 65535) >> 16
    alloc_fn.instruction(&Instruction::LocalGet(2));
    alloc_fn.instruction(&Instruction::MemorySize(0));
    alloc_fn.instruction(&Instruction::I32Const(16));
    alloc_fn.instruction(&Instruction::I32Shl);
    alloc_fn.instruction(&Instruction::I32Sub);
    alloc_fn.instruction(&Instruction::I32Const(65535));
    alloc_fn.instruction(&Instruction::I32Add);
    alloc_fn.instruction(&Instruction::I32Const(16));
    alloc_fn.instruction(&Instruction::I32ShrU);
    alloc_fn.instruction(&Instruction::MemoryGrow(0));
    alloc_fn.instruction(&Instruction::I32Const(-1));
    alloc_fn.instruction(&Instruction::I32Eq);
    alloc_fn.instruction(&Instruction::If(BlockType::Empty));
    alloc_fn.instruction(&Instruction::Unreachable); // Trap on out-of-memory
    alloc_fn.instruction(&Instruction::End);
    alloc_fn.instruction(&Instruction::End);

    // global.set 0 new_ptr
    alloc_fn.instruction(&Instruction::LocalGet(2));
    alloc_fn.instruction(&Instruction::GlobalSet(0));

    // return old_ptr
    alloc_fn.instruction(&Instruction::LocalGet(1));
    alloc_fn.instruction(&Instruction::End);

    alloc_fn
}

/// Builds the `__aipo_task_create(fn_table_idx: i32, arg_count: i32, arg0: i64, arg1: i64) -> i32` function.
pub(super) fn build_task_create_function(alloc_func_idx: u32) -> Function {
    // 1 local: local 4 (task_ptr: i32)
    let mut func = Function::new([(1, ValType::I32)]);

    // task_ptr = alloc(48)
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(4));

    // [task_ptr + 0] = status = 0 (Pending)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 4] = fn_table_idx (param 0)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 8] = result = 0 (i64)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // [task_ptr + 16] = state = 0 (i32)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 16,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 20] = arg_count (param 1)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 20,
        align: 2,
        memory_index: 0,
    }));

    // [task_ptr + 24] = arg0 (param 2)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 24,
        align: 3,
        memory_index: 0,
    }));

    // [task_ptr + 32] = arg1 (param 3)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 32,
        align: 3,
        memory_index: 0,
    }));

    // [task_ptr + 40] = next_ptr = 0 (i32)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 40,
        align: 2,
        memory_index: 0,
    }));

    // return task_ptr
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);

    func
}

/// Builds the `__aipo_task_drive(task_ptr: i32) -> i64` function.
pub(super) fn build_task_drive_function(sig_0: u32, sig_1: u32, sig_2: u32) -> Function {
    // 4 locals:
    // local 1: status (i32)
    // local 2: fn_idx (i32)
    // local 3: arg_count (i32)
    // local 4: res (i64)
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);

    // status = [task_ptr + 0]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    // if status == 2 (Ready) -> return [task_ptr + 8]
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // if status == 4 (Cancelled) -> trap (unreachable)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // mark status = 1 (Running)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // fn_idx = [task_ptr + 4]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // arg_count = [task_ptr + 20]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 20,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // Dispatch via call_indirect based on arg_count
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
    // Arity 0
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::CallIndirect {
        type_index: sig_0,
        table_index: 0,
    });
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
    // Arity 1: load arg0 from [task_ptr + 24]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 24,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::CallIndirect {
        type_index: sig_1,
        table_index: 0,
    });
    func.instruction(&Instruction::Else);
    // Arity 2: load arg0 from [task_ptr + 24], arg1 from [task_ptr + 32]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 24,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 32,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::CallIndirect {
        type_index: sig_2,
        table_index: 0,
    });
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Save result to local 4
    func.instruction(&Instruction::LocalSet(4));

    // [task_ptr + 8] = res
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // mark status = 2 (Ready)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // return res
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);

    func
}

/// Builds the `__aipo_await(task_ptr: i32) -> i64` function.
pub(super) fn build_await_function(task_drive_func_idx: u32) -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(task_drive_func_idx));
    func.instruction(&Instruction::End);
    func
}

/// Builds the `__aipo_task_sleep(duration_ms: i64) -> i64` function.
/// Advances `__aipo_virtual_time` (Global 1) by `duration_ms` and returns 0.
pub(super) fn build_task_sleep_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::GlobalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::GlobalSet(1));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds the `__aipo_task_cancel(task_ptr: i32) -> i64` function.
/// Marks the task's status as Cancelled (4) and returns 0.
pub(super) fn build_task_cancel_function() -> Function {
    let mut func = Function::new([]);
    // [task_ptr + 0] = 4 (Cancelled)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_range_new(start: i64, end: i64) -> i32`.
///
/// A range is a 16-byte heap block: `[start_lo, start_hi, end_lo, end_hi]`,
/// holding the inclusive-exclusive integer bounds produced by the `..` operator.
pub(super) fn build_range_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // local 2 is the allocated ptr
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(2));

    // store start (local 0) at offset 0
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    // store end (local 1) at offset 8
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_range_len(range_ptr: i32) -> i64`.
///
/// Returns `max(0, end - start)`, the number of steps the range yields.
pub(super) fn build_range_len_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Sub);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_range_get(range_ptr: i32, index: i32) -> i64`.
///
/// Index `0` reads `start`, any other index reads `end`.
pub(super) fn build_range_get_function() -> Function {
    let mut func = Function::new([]);
    // if index == 0 { load start } else { load end }
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I64)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_new(capacity: i32) -> i32`.
///
/// A dictionary is a heap block laid out as an 8-byte entry count header
/// followed by `capacity` fixed-width 24-byte entries (`hash: i64`,
/// `key: i64`, `value: i64`). Entries are filled in order and searched
/// linearly.
///
/// The hashed key is what lookup compares; the raw key is kept beside it so
/// `keys()` and `each key, value in dict` can hand back the value the program
/// wrote. A hash alone cannot be reversed, and a String key's raw pointer is what
/// the caller expects to receive.
///
/// `ponytail:` linear scan is `O(n)` per lookup. Replace with an open-addressing
/// table once dictionaries above a few dozen entries show up in benchmarks.
pub(super) fn build_dict_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(2, ValType::I32)]);
    // locals: 1=ptr, 2=capacity
    //
    // The block is fixed-size once allocated, and a dictionary literal asks for
    // exactly as many entries as it writes — so a later `d[k] = v` had nowhere to
    // go and wrote past the block, corrupting whatever the allocator placed next.
    // A minimum capacity of eight is the same headroom `list_new` gives.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(2));

    // bytes = 8 (header) + capacity * 24
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(1));

    // entry count starts at 0
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_set(dict: i32, key: i64, value: i64) -> i64`.
///
/// Replaces the value when the key is already present, otherwise appends a new
/// entry. Returns `value` so the call can stay in expression position.
pub(super) fn build_dict_set_function() -> Function {
    // params: 0=dict, 1=hash, 2=raw key, 3=value
    // locals: 4=index, 5=entry_count
    let mut func = Function::new([(2, ValType::I32)]);

    // entry_count = dict[0]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));

    // Scan for an existing key, remembering the first free slot.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    // block $done
    func.instruction(&Instruction::Block(BlockType::Empty));
    // loop
    func.instruction(&Instruction::Loop(BlockType::Empty));

    // if index == entry_count then leave the loop
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // entry key = dict[4 + index*16]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    // key matched: overwrite the value in place.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));
    // Return `value` immediately without appending or altering entry_count.
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // index += 1
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Append at index: hash first, then the raw key beside it.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));

    // entry_count = index + 1
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_get(dict: i32, key: i64) -> i64`.
///
/// Returns the stored value, or `none` (0) when the key is absent.
pub(super) fn build_dict_get_function() -> Function {
    // locals: 0=dict, 1=key, 2=index, 3=entry_count
    let mut func = Function::new([(2, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // A missing key is a runtime fault, not `none`: canon makes `dict[key]` require
    // an existing key so a stored `none` stays distinguishable from absence. Returning
    // 0 here made a missing key read as the integer zero (or as an empty container),
    // which silently produced a wrong value instead of the VM's
    // `AIPO_RT_KEY_NOT_FOUND`.
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_len(dict: i32) -> i64`.
pub(super) fn build_dict_len_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_has(dict: i32, key: i64) -> i32`.
///
/// Returns 1 when the key is present, 0 otherwise.
pub(super) fn build_dict_has_function() -> Function {
    // locals: 0=dict, 1=key, 2=index, 3=entry_count
    let mut func = Function::new([(2, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_is_empty(dict: i32) -> i32`.
pub(super) fn build_dict_is_empty_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_remove(dict: i32, key: i64) -> i64`.
///
/// Removes first entry matching `key` by shifting subsequent entries down.
/// Returns 1 if removed, 0 if not found.
pub(super) fn build_dict_remove_function() -> Function {
    // locals: 0=dict, 1=key, 2=index, 3=entry_count, 4=j
    let mut func = Function::new([(3, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // entry_addr = dict + 8 + index * 16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));

    // Shift entries down: j = index; while j < entry_count-1
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // copy entry j+1 key to entry j key
    // dest = dict + 8 + j*16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    // src key = dict + 8 + (j+1)*16
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    // copy entry j+1 value to entry j value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End); // loop
    func.instruction(&Instruction::End); // block

    // Decrement entry_count
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::Return);

    func.instruction(&Instruction::End); // if

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End); // loop
    func.instruction(&Instruction::End); // block

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_keys(dict: i32) -> i32` (returns a List).
pub(super) fn build_dict_keys_function(list_new_func_idx: u32, list_set_func_idx: u32) -> Function {
    // locals: 0=dict, 1=count, 2=list, 3=i
    let mut func = Function::new([(3, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(list_new_func_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // raw key = dict + 8 (header) + i*24 + 8 (entry's key slot). The base already
    // lands inside the entries, so the load carries no extra offset — adding one
    // read the entry's value instead of its key.
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Call(list_set_func_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_key_at(dict: i32, index: i32) -> i64` (returns the raw key).
/// Compiles a dictionary key onto the stack as `(hash: i64, raw: i64)`.
///
/// The hash answers lookup, and the raw value is what iteration hands back. A
/// String key's raw value is its pointer; a non-String key hashes to itself.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_dict_key_at_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_value_at(dict: i32, index: i32) -> i64`.
pub(super) fn build_dict_value_at_function() -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 16,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_dict_values(dict: i32) -> i32` (returns a List).
pub(super) fn build_dict_values_function(
    list_new_func_idx: u32,
    list_set_func_idx: u32,
) -> Function {
    // locals: 0=dict, 1=count, 2=list, 3=i
    let mut func = Function::new([(3, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(list_new_func_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // value = dict header + i*24 + offset 16
    // value = dict + 8 (header) + i*24 + 16
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(24));
    func.instruction(&Instruction::I32Mul);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 0,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Call(list_set_func_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_string_hash(ptr: i32) -> i64`.
///
/// FNV-1a 64-bit hash over the string bytes so identical strings with different
/// pointers can collide onto the same key bucket when hashing is introduced.
pub(super) fn build_string_hash_function() -> Function {
    // locals: 0=ptr, 1=hash(i64), 2=len(i32), 3=i(i32)
    let mut func = Function::new([(1, ValType::I64), (2, ValType::I32)]);

    // hash = 0xcbf29ce484222325 (FNV offset basis)
    func.instruction(&Instruction::I64Const(-3750763034362895579i64));
    func.instruction(&Instruction::LocalSet(1));

    // len = [ptr+0]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // i = 0
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // hash = (hash ^ byte) * 0x100000001b3
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Xor);
    func.instruction(&Instruction::I64Const(1099511628211i64));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));

    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_new(len: i32) -> i32`.
///
/// A list value is an 8-byte handle block `[data_ptr: i32, pad: i32]` in linear memory.
/// The `data_ptr` points to the dynamically resizable buffer `[len: i32, cap: i32, elements: [i64; cap]]`.
/// Handlers never change address across growth, ensuring reference identity is preserved.
pub(super) fn build_list_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(4, ValType::I32)]);
    // local 0: len (param)
    // local 1: cap
    // local 2: buf_size
    // local 3: buf_ptr
    // local 4: handle_ptr

    // cap = if len < 8 { 8 } else { len }
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(1));

    // buf_size = 8 + cap * 8
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));

    // buf_ptr = alloc(buf_size)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(3));

    // store len at buf_ptr + 0
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // store cap at buf_ptr + 4
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    // handle_ptr = alloc(8)
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(4));

    // store buf_ptr at handle_ptr + 0
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // return handle_ptr
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_get(list_ptr: i32, index: i32) -> i64`.
pub(super) fn build_list_get_function() -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: buf_ptr

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // bounds check: if index < 0 trap
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // bounds check: if index >= len trap
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // element address = buf_ptr + 8 + index * 8
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_len(list_ptr: i32) -> i64`.
pub(super) fn build_list_len_function() -> Function {
    let mut func = Function::new([]);
    // local 0: handle_ptr
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_set(list_ptr: i32, index: i32, value: i64) -> i64`.
pub(super) fn build_list_set_function() -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: value
    // local 3: buf_ptr

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // bounds check: if index < 0 or index >= len trap
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    // element address = buf_ptr + 8 + index * 8
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_add(list_ptr: i32, value: i64) -> i64`.
///
/// Appends an item to the list, expanding the backing buffer exponentially (2x) when full.
pub(super) fn build_list_add_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(6, ValType::I32)]);
    // local 0: handle_ptr (param)
    // local 1: val (param i64)
    // local 2: buf_ptr
    // local 3: len
    // local 4: cap
    // local 5: new_cap
    // local 6: new_buf
    // local 7: i

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len = i32.load(buf_ptr + 0)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // cap = i32.load(buf_ptr + 4)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    // if len >= cap { grow }
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));

    // new_cap = if cap <= 0 { 8 } else { cap * 2 }
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LeS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    // new_buf = alloc(8 + new_cap * 8)
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Const(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(6));

    // new_buf[0] = len
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // new_buf[4] = new_cap
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));

    // copy loop: i = 0; while i < len { new_buf[8 + i*8] = buf_ptr[8 + i*8]; i++ }
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // dst = new_buf + (i << 3)
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);

    // src = buf_ptr + (i << 3)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // i += 1
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End); // loop
    func.instruction(&Instruction::End); // block

    // handle_ptr[0] = new_buf
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // buf_ptr = new_buf
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::End); // if len >= cap

    // store val at buf_ptr + 8 + (len << 3)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));

    // update len at buf_ptr + 0 = len + 1
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_contains(list_ptr: i32, value: i64) -> i32`.
pub(super) fn build_list_contains_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: value
    // local 2: buf_ptr
    // local 3: len
    // local 4: i

    // buf_ptr = i32.load(handle_ptr + 0)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len = i32.load(buf_ptr + 0)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_insert(list_ptr: i32, index: i32, value: i64) -> i64`.
pub(super) fn build_list_insert_function(
    list_add_idx: u32,
    list_len_idx: u32,
    list_get_idx: u32,
    list_set_idx: u32,
) -> Function {
    let mut func = Function::new([(2, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: value
    // local 3: old_len
    // local 4: i

    // Append a dummy to grow and update len, then shift right.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::BrIf(1));

    // list[i + 1] = list[i]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::Call(list_set_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // list[index] = value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(list_set_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_remove_last(list_ptr: i32) -> i64`.
pub(super) fn build_list_remove_last_function() -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    // local 0: handle_ptr
    // local 1: buf_ptr
    // local 2: len
    // local 3: index
    // local 4: value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_clear(list_ptr: i32) -> i64`.
pub(super) fn build_list_clear_function() -> Function {
    let mut func = Function::new([(1, ValType::I32)]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_remove_at(list_ptr: i32, index: i32) -> i64`.
pub(super) fn build_list_remove_at_function() -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    // local 0: handle_ptr
    // local 1: index
    // local 2: buf_ptr
    // local 3: len
    // local 4: i
    // local 5: value
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Unreachable);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_remove(list_ptr: i32, value: i64) -> i64`.
pub(super) fn build_list_remove_function(list_remove_at_idx: u32) -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: handle_ptr
    // local 1: value
    // local 2: buf_ptr
    // local 3: len
    // local 4: i
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_remove_at_idx));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_count(list_ptr: i32, value: i64) -> i64`.
pub(super) fn build_list_count_function() -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_find(list_ptr: i32, value: i64) -> i64`.
pub(super) fn build_list_find_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: handle, local 1: value, local 2: buf, local 3: len, local 4: i
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I64Const(-1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64ExtendI32S);
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::I64Const(-1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_reverse(list_ptr: i32) -> i64`.
pub(super) fn build_list_reverse_function() -> Function {
    let mut func = Function::new([(4, ValType::I32), (1, ValType::I64)]);
    // local 0: handle, local 1: buf, local 2: len, local 3: i, local 4: j, local 5: tmp
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    // Reversing mutates in place and yields the same list handle, so the result
    // keeps its `i32` representation and can be chained.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_transform(list_ptr: i32, callback: i32) -> i32`.
pub(super) fn build_list_transform_function(
    list_new_idx: u32,
    list_len_idx: u32,
    list_get_idx: u32,
    list_set_idx: u32,
    callback_type_idx: u32,
) -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: list
    // local 1: callback table index
    // local 2: new list
    // local 3: len
    // local 4: i

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::Call(list_set_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_filter(list_ptr: i32, callback: i32) -> i32`.
pub(super) fn build_list_filter_function(
    list_new_idx: u32,
    list_len_idx: u32,
    list_get_idx: u32,
    list_add_idx: u32,
    callback_type_idx: u32,
) -> Function {
    let mut func = Function::new([(3, ValType::I32), (1, ValType::I64)]);
    // local 0: list
    // local 1: callback table index
    // local 2: new list
    // local 3: len
    // local 4: i
    // local 5: item

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_sort(list_ptr: i32) -> i32` (signed i64 insertion sort).
pub(super) fn build_list_sort_function() -> Function {
    let mut func = Function::new([(4, ValType::I32), (1, ValType::I64)]);
    // local 0: list
    // local 1: buf
    // local 2: len
    // local 3: i
    // local 4: j
    // local 5: key
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_sort_by(list_ptr: i32, callback: i32) -> i32`.
pub(super) fn build_list_sort_by_function(callback_type_idx: u32) -> Function {
    let mut func = Function::new([(4, ValType::I32), (3, ValType::I64)]);
    // local 0: list
    // local 1: callback
    // local 2: buf
    // local 3: len
    // local 4: i
    // local 5: j
    // local 6: key_i
    // local 7: key_i_sort
    // local 8: key_j_sort
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::CallIndirect {
        type_index: callback_type_idx,
        table_index: 0,
    });
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64GtS);
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::I32Shl);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I64Store(MemArg {
        offset: 8,
        align: 3,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_concat(ptr_a: i32, ptr_b: i32) -> i32`.
pub(super) fn build_string_concat_function(alloc_func_idx: u32, nfc_func_idx: u32) -> Function {
    let mut func = Function::new([(5, ValType::I32)]);
    // local 0: ptr_a (param)
    // local 1: ptr_b (param)
    // local 2: len_a
    // local 3: len_b
    // local 4: total_len
    // local 5: new_ptr
    // local 6: i

    // len_a = i32.load(ptr_a)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len_b = i32.load(ptr_b)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // total_len = len_a + len_b
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));

    // new_ptr = alloc(total_len + 4)
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(5));

    // store total_len at new_ptr + 0
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // copy bytes from a: loop i from 0 to len_a - 1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // copy bytes from b: loop i from 0 to len_b - 1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    // `+` on strings is NFC on the VM, so concatenation normalizes its result.
    func.instruction(&Instruction::Call(nfc_func_idx));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_int_to_str(val: i64) -> i32`.
/// Builds `__aipo_str_to_int(ptr: i32) -> i64`.
/// Prefix of the message `Int(text)` reports for text that is not a number.
pub(super) const INT_PARSE_PREFIX: &str = "invalid integer text: \"";

/// Prefix and suffix of the message a violated `invariant()` reports, so the
/// handler sees the same wording the VM produces.
pub(super) const INVARIANT_PREFIX: &str = "invariant() of ";
pub(super) const INVARIANT_SUFFIX: &str = " did not hold after mutation";

pub(super) fn build_str_to_int_function(
    failure_status_global: u32,
    failure_message_global: u32,
    concat_func_idx: u32,
    prefix_ptr: i32,
    quote_ptr: i32,
) -> Function {
    // locals: 0=ptr (param), 1=len, 2=i, 3=result(i64), 4=sign, 5=digit, 6=started
    let mut func = Function::new([(2, ValType::I32), (2, ValType::I64), (2, ValType::I32)]);
    // local 0: ptr
    // local 1: len (i32)
    // local 2: i (i32)
    // local 3: result (i64)
    // local 4: sign (i64, 1 or -1)
    // local 5: digit (i32)
    // local 6: started (i32)

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I64Const(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(6));

    // skip leading spaces
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(32));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // sign
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(45)); // '-'
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I64Const(-1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // parse digits
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(9));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64Mul);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalSet(3));
    // Canon makes `Int(text)` a `Failure` when the text is not a number, so a
    // `or_else` fallback and an `attempt` handler both have to see it. The message
    // quotes the offending text, matching the wording the VM produces, so the
    // handler sees the same message on every backend.
    func.instruction(&Instruction::I32Const(prefix_ptr));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(concat_func_idx));
    func.instruction(&Instruction::I32Const(quote_ptr));
    func.instruction(&Instruction::Call(concat_func_idx));
    func.instruction(&Instruction::GlobalSet(failure_message_global));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::GlobalSet(failure_status_global));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_case(ptr: i32, upper: i32) -> i32`.
///
/// Case folding over code points. Each character is decoded, folded, and written
/// into a fresh buffer. ASCII folds arithmetically and the Latin-1 Supplement
/// (`U+00C0..=U+00DE` / `U+00E0..=U+00FE`, excluding `×` and `÷`) folds by 0x20,
/// which covers the accented letters a program is likely to meet. A code point
/// outside those ranges is copied unchanged rather than corrupted.
///
/// Folding never changes the UTF-8 width of those ranges, so the output is the same
/// size as the input and no buffer growth is needed.
pub(super) fn build_str_find_function() -> Function {
    // params: 0=haystack, 1=needle
    // locals: 2=h_len, 3=n_len, 4=i, 5=j, 6=byte, 7=hit
    let mut func = Function::new([(6, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::If(BlockType::Empty));
    // Report the byte index of the match, widened to the `i64` the caller reads.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I64ExtendI32S);
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::End);
    func
}

/// Copies `len` bytes from `src + src_off` into a fresh pool string.
///
/// `strings` are `[len: i32][bytes]`, so a builder needs the byte count and the
/// source offset rather than the `(start, end)` code-point window a slice takes.
pub(super) fn build_str_copy_function(alloc_func_idx: u32) -> Function {
    // params: 0=src, 1=src_off, 2=len
    // locals: 3=out, 4=i, 5=byte
    let mut func = Function::new([(3, ValType::I32)]);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_reverse(ptr: i32, copy_idx: u32) -> i32`.
///
/// Reverses by code point, so a multi-byte character keeps its byte order.
pub(super) fn build_str_reverse_function(
    copy_idx: u32,
    alloc_func_idx: u32,
    nfc_func_idx: u32,
) -> Function {
    // params: 0=ptr
    // locals: 1=len, 2=out, 3=cursor(dst), 4=pos, 5=char_start, 6=byte, 7=step, 8=i
    let mut func = Function::new([(8, ValType::I32)]);
    let _ = copy_idx;

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // Walk the source forward once, recording each code-point start so the second
    // pass can replay the boundaries from the end. A reverse over raw bytes would
    // cut a multi-byte sequence in half.
    let mut starts: Vec<u32> = Vec::new();
    let _ = &mut starts;
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    // char_start = pos; advance pos past one whole character.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Second pass: walk the same boundaries backwards, copying each character to
    // the output cursor in reverse order.
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    // Retreat to the start of the character that ends at `pos`.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(0xC0));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Copy bytes [5, 4) to the output cursor.
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    // Reversing moves combining marks in front of their base, so the result is
    // re-normalized like `string.reverse` on the VM.
    func.instruction(&Instruction::Call(nfc_func_idx));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_split(text: i32, sep: i32, list_new: u32, list_set: u32) -> i32`.
///
/// Splits on every occurrence of a non-empty separator and returns a List of the
/// pieces, in order. The separator itself is never part of a piece.
/// Builds `__aipo_str_format(template: i32, dict: i32) -> i32`.
///
/// Replaces `{name}` placeholders with the dictionary entry named by `name`. A
/// `{{` escape emits a literal brace. Values render as their stored `i64`.
pub(super) fn build_str_format_function(
    concat_idx: u32,
    copy_idx: u32,
    hash_idx: u32,
    dict_get_idx: u32,
    int_to_str_idx: u32,
) -> Function {
    // params: 0=template, 1=dict
    // locals: 2=len, 3=out, 4=pos, 5=scan, 6=ch, 7=piece_start, 8=key, 9=value
    let mut func = Function::new([(9, ValType::I32), (1, ValType::I64)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(8));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(7));

    // An interior `{` opens a placeholder; `{{` is an escaped literal brace.
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(123));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    let escaped = 1u32;
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(123));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    // Flush [piece_start, pos) then one brace.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Br(escaped + 2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // A placeholder: the key runs from pos+1 to the closing `}`.
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Const(125));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // key = template[pos+1 .. scan]
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::LocalSet(9));

    // Flush the piece before the placeholder, then the looked-up value.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::Call(hash_idx));
    func.instruction(&Instruction::Call(dict_get_idx));
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::LocalGet(12));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalGet(12));
    func.instruction(&Instruction::Call(int_to_str_idx));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::Select);
    func.instruction(&Instruction::LocalSet(11));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Br(1));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Flush the trailing piece.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_join(sep: i32, list: i32) -> i32`.
///
/// Concatenates every element (a String pointer) with `sep` between them.
pub(super) fn build_str_join_function(
    list_len_idx: u32,
    list_get_idx: u32,
    concat_idx: u32,
) -> Function {
    // params: 0=sep, 1=list
    // locals: 2=len, 3=i, 4=acc, 5=elem
    let mut func = Function::new([(4, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(2));

    // Start from the empty string so the first concatenation has a left operand.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_replace(text: i32, from: i32, to: i32, ...) -> i32`.
///
/// Replaces every non-overlapping occurrence of `from`. The piece before each
/// match and the replacement are concatenated onto the accumulator.
pub(super) fn build_str_replace_function(copy_idx: u32, concat_idx: u32) -> Function {
    // params: 0=text, 1=from, 2=to
    // locals: 3=len, 4=from_len, 5=pos, 6=acc, 7=i, 8=match, 9=piece_start
    let mut func = Function::new([(7, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(6));

    // An empty pattern has no meaningful replacement, so the text is unchanged.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::If(BlockType::Empty));
    // text[piece_start..5] + to
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(1));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // The tail after the last match.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::Call(concat_idx));
    func.instruction(&Instruction::End);
    func
}

pub(super) fn build_str_split_function(
    list_new_idx: u32,
    list_add_idx: u32,
    copy_idx: u32,
) -> Function {
    // params: 0=text, 1=sep
    // locals: 2=len, 3=sep_len, 4=piece_start, 5=pos, 6=list, 7=count, 8=i, 9=match
    let mut func = Function::new([(8, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // A fresh list starts empty: the length is the element count, so the capacity
    // argument here must be zero and not the initial capacity's minimum.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(6));

    // An empty separator has no meaningful split, so the whole text is one piece.
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::BrIf(1));

    // Compare the separator against `text[5..5+sep_len]`.
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(9));
    func.instruction(&Instruction::Br(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(8));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::If(BlockType::Empty));
    // A match closes the current piece at `5` and resumes past the separator.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(1));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // The trailing piece runs from `4` to the end.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::Call(copy_idx));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_starts(haystack: i32, needle: i32) -> i32`.
///
/// Returns 1 when `haystack` begins with `needle`. `__aipo_str_ends` is the same
/// comparison anchored at the tail.
pub(super) fn build_str_affix_function(at_end: bool) -> Function {
    // params: 0=haystack, 1=needle
    // locals: 2=h_len, 3=n_len, 4=offset, 5=i, 6=byte
    let mut func = Function::new([(5, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // A needle longer than the haystack cannot be an affix.
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    if at_end {
        func.instruction(&Instruction::LocalGet(2));
        func.instruction(&Instruction::LocalGet(3));
        func.instruction(&Instruction::I32Sub);
        func.instruction(&Instruction::LocalSet(4));
    } else {
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(4));
    }

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_trim(ptr: i32) -> i32`.
///
/// Drops leading and trailing whitespace, returning a fresh string. Only the ends
/// are trimmed, so an interior run is preserved.
pub(super) fn build_float_to_str_function(
    alloc_func_idx: u32,
    int_to_str_idx: u32,
    str_concat_idx: u32,
) -> Function {
    // param 0 = value (f64)
    // Two floats follow it, then the integer scratch slots:
    //   1,2 = int_part, frac
    //   3..7 = out, count, handle, digit, parked
    const INT_PART: u32 = 1;
    const FRAC: u32 = 2;
    const OUT: u32 = 3;
    const COUNT: u32 = 4;
    const HANDLE: u32 = 5;
    const DIGIT: u32 = 6;
    const PARKED: u32 = 7;

    let mut func = Function::new([(2, ValType::F64), (5, ValType::I32)]);

    // out = int_to_str(trunc(value))
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::F64Trunc);
    func.instruction(&Instruction::I64TruncF64S);
    func.instruction(&Instruction::Call(int_to_str_idx));
    func.instruction(&Instruction::LocalSet(OUT));

    // int_part = trunc(value); frac = value - int_part (non-negative for our use).
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::F64Trunc);
    func.instruction(&Instruction::LocalSet(INT_PART));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(INT_PART));
    func.instruction(&Instruction::F64Sub);
    func.instruction(&Instruction::LocalSet(FRAC));

    // Append '.'.
    func.instruction(&Instruction::LocalGet(OUT));
    one_char_string(&mut func, alloc_func_idx, HANDLE, b'.');
    func.instruction(&Instruction::Call(str_concat_idx));
    func.instruction(&Instruction::LocalSet(OUT));

    // An integral value stops at one decimal digit, matching the VM.
    func.instruction(&Instruction::LocalGet(FRAC));
    func.instruction(&Instruction::F64Const(0.0.into()));
    func.instruction(&Instruction::F64Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(OUT));
    one_char_string(&mut func, alloc_func_idx, HANDLE, b'0');
    func.instruction(&Instruction::Call(str_concat_idx));
    func.instruction(&Instruction::LocalSet(OUT));
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // Emit fraction digits, stopping once the remainder is exhausted.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(COUNT));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(COUNT));
    // A double carries at most sixteen significant decimal digits, which is what
    // the VM's shortest round-tripping form produces (`1/3` is sixteen threes).
    func.instruction(&Instruction::I32Const(16));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(FRAC));
    func.instruction(&Instruction::F64Const(10.0.into()));
    func.instruction(&Instruction::F64Mul);
    func.instruction(&Instruction::LocalSet(FRAC));
    // digit = trunc(frac); frac -= digit
    func.instruction(&Instruction::LocalGet(FRAC));
    func.instruction(&Instruction::F64Trunc);
    func.instruction(&Instruction::I32TruncF64S);
    func.instruction(&Instruction::LocalSet(DIGIT));
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::LocalGet(DIGIT));
    func.instruction(&Instruction::I32Const(b'0' as i32));
    func.instruction(&Instruction::I32Add);
    one_char_value(&mut func, alloc_func_idx, HANDLE, PARKED);
    func.instruction(&Instruction::Call(str_concat_idx));
    func.instruction(&Instruction::LocalSet(OUT));
    func.instruction(&Instruction::LocalGet(FRAC));
    func.instruction(&Instruction::LocalGet(DIGIT));
    func.instruction(&Instruction::F64ConvertI32S);
    func.instruction(&Instruction::F64Sub);
    func.instruction(&Instruction::LocalSet(FRAC));
    func.instruction(&Instruction::LocalGet(FRAC));
    func.instruction(&Instruction::F64Const(0.0.into()));
    func.instruction(&Instruction::F64Le);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(COUNT));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(COUNT));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::End);
    func
}

/// Emits a one-character ASCII string for `byte`, leaving its handle on the stack.
fn one_char_string(func: &mut Function, alloc_func_idx: u32, handle: u32, byte: u8) {
    func.instruction(&Instruction::I32Const(5));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalTee(handle));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(handle));
    func.instruction(&Instruction::I32Const(i32::from(byte)));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(handle));
}

/// Wraps the byte code on the stack into a one-character string.
fn one_char_value(func: &mut Function, alloc_func_idx: u32, handle: u32, parked: u32) {
    func.instruction(&Instruction::LocalSet(parked));
    func.instruction(&Instruction::I32Const(5));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalTee(handle));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(handle));
    func.instruction(&Instruction::LocalGet(parked));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(handle));
}

pub(super) fn build_int_to_str_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(5, ValType::I32), (2, ValType::I64)]);
    // local 0: val (param, i64)
    // local 1: is_neg (i32)
    // local 2: digits_len (i32)
    // local 3: total_len (i32)
    // local 4: ptr (i32)
    // local 5: idx (i32)
    // local 6: uval (i64)
    // local 7: temp (i64)

    // Check if val < 0
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::I64LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::I64Const(0));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I64Sub);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::End);

    // If uval == 0, digits_len = 1
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(7));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64DivU);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // total_len = digits_len + is_neg
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));

    // ptr = __aipo_alloc(total_len + 4)
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(4));

    // store total_len at ptr + 0
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // If is_neg == 1, store '-' (45) at ptr + 4
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(45));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);

    // If uval == 0, store '0' (48) at ptr + 4 + is_neg
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalSet(7));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Eqz);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64RemU);
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::I32Const(48));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I64Const(10));
    func.instruction(&Instruction::I64DivU);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bool_to_str(val: i32) -> i32`.
pub(super) fn build_bool_to_str_function(true_ptr: i32, false_ptr: i32) -> Function {
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(true_ptr));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(false_ptr));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_eq(ptr_a: i32, ptr_b: i32) -> i32`.
pub(super) fn build_string_eq_function() -> Function {
    let mut func = Function::new([(3, ValType::I32)]);
    // local 0: ptr_a (param)
    // local 1: ptr_b (param)
    // local 2: len_a
    // local 3: len_b
    // local 4: i

    // if ptr_a == ptr_b return 1
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Eq);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // len_a = i32.load(ptr_a)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // len_b = i32.load(ptr_b)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // if len_a != len_b return 0
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // loop i from 0 to len_a - 1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_new(len: i32) -> i32`.
pub(super) fn build_bytes_new_function(alloc_func_idx: u32) -> Function {
    let mut func = Function::new([(2, ValType::I32)]);
    // local 0: len (param)
    // local 1: ptr
    // local 2: i

    // alloc size = len + 4
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(1));

    // store len at ptr + 0
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // Zero out bytes: loop i = 0..len-1
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_get(buf_ptr: i32, index: i32) -> i64`.
pub(super) fn build_bytes_get_function() -> Function {
    let mut func = Function::new([]);
    // local 0: ptr (i32)
    // local 1: index (i32)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I64Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_set(buf_ptr: i32, index: i32, val: i64) -> i64`.
pub(super) fn build_bytes_set_function() -> Function {
    let mut func = Function::new([]);
    // local 0: ptr (i32)
    // local 1: index (i32)
    // local 2: val (i64)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_len(buf_ptr: i32) -> i64`.
pub(super) fn build_bytes_len_function() -> Function {
    let mut func = Function::new([]);
    // local 0: ptr (i32)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_bytes_slice(buf: i32, start: i32, end: i32) -> i32`.
///
/// Returns a fresh managed block holding `buf[start..end]`, clamped to the source
/// length. `Bytes` is mutable and by value in canon, so a slice must not alias the
/// original: writing to the window cannot disturb the base block.
pub(super) fn build_bytes_slice_function(alloc_func_idx: u32) -> Function {
    // locals: 0=buf, 1=start, 2=end, 3=len, 4=clamped_start, 5=clamped_end,
    //         6=out, 7=i
    let mut func = Function::new([(5, ValType::I32)]);

    // len = load(buf)
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // clamped_start = clamp(start, 0, len)
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(4));

    // clamped_end = clamp(end, clamped_start, len)
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    // out = alloc(4 + (clamped_end - clamped_start))
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(6));

    // store len at out + 0
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // copy loop
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_slice(list_ptr: i32, start: i32, end: i32) -> i32`.
///
/// Returns a fresh list holding `list[start..end]`, with both bounds clamped into
/// `0..=len`. Canon slicing is tolerant, so an out-of-range bound narrows to the
/// nearest valid edge instead of trapping.
pub(super) fn build_list_slice_function(
    list_new_idx: u32,
    list_add_idx: u32,
    list_get_idx: u32,
) -> Function {
    // params: 0=list, 1=start, 2=end
    // locals: 3=len, 4=lo, 5=hi, 6=out, 7=i
    let mut func = Function::new([(5, ValType::I32)]);
    const LEN: u32 = 3;
    const LO: u32 = 4;
    const HI: u32 = 5;
    const OUT: u32 = 6;
    const I: u32 = 7;

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(LEN));

    // lo = clamp(start, 0, len), a negative bound counting from the end.
    //
    // Canon slicing normalizes negatives (`xs[-2..]` is the last two elements) and
    // clamps out-of-range bounds instead of faulting. Treating a negative bound as
    // zero made every negative slice return the whole list.
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(1));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(LO));

    // hi = clamp(end, lo, len), a negative bound counting from the end.
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(LO));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(LO));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::I32GtS);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(HI));

    // out = list_new(0), then grow through `list_add`. `list_new(n)` already
    // reports `n` as the length, so passing the window size here and then
    // appending would double it.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(OUT));

    func.instruction(&Instruction::LocalGet(LO));
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::LocalGet(HI));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    // out = list.add(out, list_get(list, i))
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);

    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_list_concat(a: i32, b: i32) -> i32`.
///
/// Returns a fresh list holding every element of `a` followed by every element of
/// `b`. Canon's `+` on two lists is a concatenation, not an addition of handles;
/// adding the pointers instead produced a value that was not a list at all.
pub(super) fn build_list_concat_function(
    list_new_idx: u32,
    list_add_idx: u32,
    list_get_idx: u32,
    list_len_idx: u32,
) -> Function {
    // params: 0=a, 1=b
    // locals: 2=out, 3=i, 4=len
    let mut func = Function::new([(3, ValType::I32)]);
    const OUT: u32 = 2;
    const I: u32 = 3;
    const LEN: u32 = 4;

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::Call(list_new_idx));
    func.instruction(&Instruction::LocalSet(OUT));

    // First operand.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(LEN));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // Second operand.
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::Call(list_len_idx));
    func.instruction(&Instruction::I32WrapI64);
    func.instruction(&Instruction::LocalSet(LEN));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::LocalGet(LEN));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::Call(list_get_idx));
    func.instruction(&Instruction::Call(list_add_idx));
    func.instruction(&Instruction::Drop);
    func.instruction(&Instruction::LocalGet(I));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(I));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(OUT));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_char(ptr: i32, index: i32) -> i32`.
///
/// Returns a fresh one-character string holding the code point at `index`. The
/// index is a *code point* position in canon, but the pool stores UTF-8 bytes, so
/// the walk advances by whole encoded characters and an out-of-range index yields
/// the empty string instead of trapping.
pub(super) fn build_str_char_function(alloc_func_idx: u32) -> Function {
    // params: 0=ptr, 1=index
    // locals: 2=len, 3=i, 4=cursor, 5=step, 6=byte, 7=out
    let mut func = Function::new([(6, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(2));

    // Walk forward `index` characters to find the byte offset of the target.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));

    // UTF-8 leading byte decides the encoded length.
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    // A cursor past the end means the index named no character.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::Return);
    func.instruction(&Instruction::End);

    // The walk above only sets `step` while it advances, so index `0` reaches
    // here with no length recorded. Measure the character at the cursor directly.
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(5));

    // out = alloc(4 + step)
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(7));

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    // Copy the encoded character byte by byte.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_slice(ptr: i32, start: i32, end: i32) -> i32`.
///
/// Slices by code point, clamping both bounds into `0..=len`, and returns a fresh
/// string. Slicing by byte offset would cut multi-byte characters in half.
pub(super) fn build_str_slice_function(alloc_func_idx: u32) -> Function {
    // params: 0=ptr, 1=start, 2=end
    // locals: 3=len, 4=i, 5=cursor, 6=step, 7=byte, 8=lo, 9=hi, 10=out, 11=n,
    //         12=code points
    let mut func = Function::new([(10, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(3));

    // Count code points, then normalize both bounds against that count.
    //
    // A negative bound counts from the end, and canonical slicing clamps rather
    // than faulting, so `word[-2..99]` is the last two characters instead of an
    // empty window. The walk below compares code-point indices, so the count has
    // to be in code points and not in bytes.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(12));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(12));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    for bound in [1u32, 2u32] {
        func.instruction(&Instruction::LocalGet(bound));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::LocalGet(12));
        func.instruction(&Instruction::LocalGet(bound));
        func.instruction(&Instruction::I32Add);
        func.instruction(&Instruction::LocalSet(bound));
        func.instruction(&Instruction::LocalGet(bound));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::I32LtS);
        func.instruction(&Instruction::If(BlockType::Empty));
        func.instruction(&Instruction::I32Const(0));
        func.instruction(&Instruction::LocalSet(bound));
        func.instruction(&Instruction::End);
        func.instruction(&Instruction::End);
    }

    // Walk to the start bound, recording the byte offset.
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(8));

    // Walk on to the end bound.
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(7));
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xE0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(2));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::LocalGet(7));
    func.instruction(&Instruction::I32Const(0xF0));
    func.instruction(&Instruction::I32LtU);
    func.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    func.instruction(&Instruction::I32Const(3));
    func.instruction(&Instruction::Else);
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalSet(6));
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalGet(6));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(5));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::LocalGet(5));
    func.instruction(&Instruction::LocalSet(9));

    // n = hi - lo; an inverted window is empty rather than a trap.
    func.instruction(&Instruction::LocalGet(9));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Sub);
    func.instruction(&Instruction::LocalSet(11));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::I32LtS);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(11));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Const(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::Call(alloc_func_idx));
    func.instruction(&Instruction::LocalSet(10));
    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32Store(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::LocalGet(11));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));
    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(8));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::I32Store8(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(4));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(10));
    func.instruction(&Instruction::End);
    func
}

/// Builds `__aipo_str_len(ptr: i32) -> i64`.
///
/// Counts code points rather than UTF-8 bytes: only bytes that are not
/// continuation bytes (`0b10xxxxxx`) start a character.
pub(super) fn build_str_len_function() -> Function {
    // params: 0=ptr
    // locals: 1=bytes, 2=i, 3=count, 4=byte
    let mut func = Function::new([(4, ValType::I32)]);

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::I32Load(MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(1));

    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::I32Const(0));
    func.instruction(&Instruction::LocalSet(3));

    func.instruction(&Instruction::Block(BlockType::Empty));
    func.instruction(&Instruction::Loop(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I32GeS);
    func.instruction(&Instruction::BrIf(1));

    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::I32Load8U(MemArg {
        offset: 4,
        align: 0,
        memory_index: 0,
    }));
    func.instruction(&Instruction::LocalSet(4));

    // 0b1100_0000 == 0xC0 marks a continuation byte; only leading bytes count.
    func.instruction(&Instruction::LocalGet(4));
    func.instruction(&Instruction::I32Const(0xC0));
    func.instruction(&Instruction::I32And);
    func.instruction(&Instruction::I32Const(0x80));
    func.instruction(&Instruction::I32Ne);
    func.instruction(&Instruction::If(BlockType::Empty));
    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(3));
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(2));
    func.instruction(&Instruction::I32Const(1));
    func.instruction(&Instruction::I32Add);
    func.instruction(&Instruction::LocalSet(2));
    func.instruction(&Instruction::Br(0));
    func.instruction(&Instruction::End);
    func.instruction(&Instruction::End);

    func.instruction(&Instruction::LocalGet(3));
    func.instruction(&Instruction::I64ExtendI32U);
    func.instruction(&Instruction::End);
    func
}
