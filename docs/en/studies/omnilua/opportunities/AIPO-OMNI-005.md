# AIPO-OMNI-005 — register windows in RegVm (call-frame optimization)

- **Status:** DONE & CERTIFIED (Phase 2 / P1 pilot 1)
- **Origin / reference:** omniLua & Lua 5.x (`CallInfo` and registers mapped to a contiguous
  execution-stack window)
- **Classification:** `ADAPT` (P1, score +18)

---

## 1. Problem identified in Aipo

1. **Blind 4 KiB copy per function activation:** in
   `crates/aipo-vm/src/reg_vm.rs:invoke_with_upvalues`, every user-function call on the
   register machine executed:
   ```rust
   let saved_registers =
       std::mem::replace(&mut self.registers, std::array::from_fn(|_| Value::None));
   ```
   and on return:
   ```rust
   self.registers = saved_registers;
   ```
   Since `self.registers` is a fixed `[Value; 256]` array (each `Value` occupying 16 bytes),
   this generated:
   - 4,096 bytes (4 KiB) copied to the Rust stack on call;
   - 256 iterations writing `Value::None` to zero the machine;
   - 4,096 bytes (4 KiB) moved back on return;
   - an excessive Rust stack footprint (64 × 4 KiB = 256 KiB at the recursion depth limit).
2. **Waste in low-arity functions:** in the vast majority of real programs (and recursive
   functions such as Fibonacci or mergesort), compiled functions use only 2 to 6 local
   registers (`num_registers` recorded in `RegCompiledFunction`). Copying 256 registers when
   only 2 or 3 were modified represented more than 98% useless overhead per activation frame.

---

## 2. Implementation performed

### A. Register-window stack (`register_stack: Vec<Value>`)
Instead of saving and zeroing the full 256-register array, `RegVm` now uses a dynamic stack of
preserved windows:
- Each compiled function declares its exact number of required registers
  (`num_registers = max_reg + 1`).
- The size of the active window to preserve for the caller is computed precisely:
  ```rust
  let callee_regs = func.num_registers.max(func.arity).max(args.len()).min(256);
  ```
- Only the first `callee_regs` registers of `self.registers` are pushed onto
  `self.register_stack`:
  ```rust
  let window_start = self.register_stack.len();
  self.register_stack.extend_from_slice(&self.registers[..callee_regs]);
  ```
- The callee is initialized only in the registers it actually uses:
  - `0..args.len()`: receive the arguments;
  - `args.len()..func.arity`: receive `Value::Unset` for missing parameters;
  - `func.arity..callee_regs`: zeroed to `Value::None` (preventing residual temporary leakage
    from the caller).
- Registers above `callee_regs` up to 255 remain untouched in `self.registers`.

### B. Surgical restoration and full cooperative-suspension support
- **On normal return:** only the `callee_regs` window is restored from the stack into
  `self.registers`:
  ```rust
  let saved_slice = &self.register_stack[window_start..];
  self.registers[..callee_regs].clone_from_slice(saved_slice);
  self.register_stack.truncate(window_start);
  ```
  No additional heap allocations during subsequent executions (the `Vec`'s internal buffer is
  reused), and no 4 KiB copies.
- **On runtime faults:** the caller's window is restored with the same guarantee, preserving
  machine-state consistency even if the callee raises division by zero or a contract violation.
- **On cooperative task suspension (`RegSuspendCapture`):** because the caller's window is
  restored before inspecting `self.pending_capture`, the `self.registers` snapshot inserted into
  the `RegFrameSlot` chain contains 100% of the caller's registers exactly as the cooperative
  scheduler expects, preserving `await`, `sleep` and `yield` semantics.

### C. Strict safety (`#![forbid(unsafe_code)]`)
- The optimization was implemented in 100% safe Rust with indexed slices and statically checked
  bounds.
- No `unsafe` line was added.

---

## 3. Quality and conformance evidence

### A. Dedicated test battery (`crates/aipo-vm/tests/reg_window_tests.rs`)
6 automated scenarios validate window correctness:
1. `test_reg_window_caller_registers_preserved_outside_callee_window`: caller with 10 registers
   calls a 2-register callee; registers outside the window preserved intact.
2. `test_reg_window_asymmetric_nested_call_cascade`: chain of calls with asymmetric windows
   (caller: 8 regs → fn1: 3 regs → fn2: 6 regs → fn3: 2 regs) unwinds and restores precisely.
3. `test_reg_window_recursive_fibonacci`: recursive Fibonacci `fib(10) = 55` on `RegVm` runs
   dozens of recursion frames without corrupting registers and drains 100% of
   `register_stack`.
4. `test_reg_window_uninitialized_locals_isolation`: guarantees that uninitialized callee
   temporaries receive `Value::None` and no caller residue.
5. `test_reg_window_restores_cleanly_on_callee_runtime_fault`: division by zero inside the
   callee restores the caller's registers and drains the stack.
6. `test_reg_window_cooperative_task_suspend_and_resume`: asynchronous activation with `await`
   suspends and resumes perfectly over the register machine.

### B. Memory-leak canaries (`leak_canaries.rs`)
- The `RegVm` lifecycle canary in `crates/aipo-testkit/tests/leak_canaries.rs` passed with
  **`growth = 0 bytes`**.

---

## 4. Measurable gains

| Metric | Before (blind copy) | After (register window) | Improvement |
|---|---|---|---|
| Memory copy per activation (3-reg callee) | 4,096 B (4 KiB) | 48 B | **-98.8%** |
| `Value::None` initializations per activation | 256 values | 0 to 3 values | **-98.8%** |
| Rust stack footprint at max recursion (64 levels) | 262,144 B (256 KiB) | ~3 KiB | **-98.8%** |
| Heap allocation overhead post-warmup | 0 allocations | 0 allocations | Identical (Zero) |
