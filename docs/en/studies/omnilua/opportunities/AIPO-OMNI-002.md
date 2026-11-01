# AIPO-OMNI-002 — instruction budget & Wasm fuel metering (uncatchable sticky fault)

- **Status:** DONE & CERTIFIED (Phase 1 / P0 pilot 2)
- **Origin / reference:** omniLua (`crates/lua-vm/src/state.rs:SandboxLimits` and
  `docs/SANDBOXING_EXPLORATION.md`)
- **Classification:** `ADAPT` (P0, score +37)

---

## 1. Problem identified in Aipo

1. **Wasmtime JIT without fuel metering:** executing WebAssembly through `aipo-wasm` (Wasmtime JIT)
   did not configure `consume_fuel(true)` nor limit fuel in the `wasmtime::Store`. Infinite loops in
   Wasm froze the process indefinitely.
2. **No unified CLI interface:** `aipo-cli` did not expose flags to govern execution budget
   (`--budget` / `--max-instructions`) uniformly across backends.
3. **Uncatchability guarantee:** per Model B+ semantics, runtime faults (`VmFault::Overflow`) must
   never be captured by `attempt { ... }` blocks, unwinding the stack unconditionally to the host.

---

## 2. Implementation performed

### A. Wasm substrate (`crates/aipo-wasm`)
- In `crates/aipo-wasm/src/runner.rs`:
  - Implemented `execute_wasm_with_fuel(wasm_bytes: &[u8], stdout: &mut dyn Write, fuel: Option<u64>)`.
  - Configured `wasmtime::Config::consume_fuel(true)` and `store.set_fuel(limit)` when `fuel.is_some()`.
  - Mapped Wasmtime fuel-exhaustion trap messages (`all fuel consumed by WebAssembly`,
    `out of fuel`) to Aipo's canonical diagnostic:
    `[AIPO_RT_OVERFLOW] runtime fault [AIPO_RT_OVERFLOW]: execution budget exceeded: fuel limit reached`.
  - `execute_wasm` preserved as a wrapper calling `execute_wasm_with_fuel(..., None)`.
- Exported in `crates/aipo-wasm/src/lib.rs`.
- Added adversarial test in `crates/aipo-wasm/tests/pipeline_tests.rs`:
  `test_wasm_fuel_budget_halts_infinite_loop`.

### B. Unified CLI interface (`crates/aipo-cli`)
- In `crates/aipo-cli/src/lib.rs`:
  - `USAGE` updated documenting `--budget=<N>` and `--max-instructions=<N>`.
  - `Command::Run` and `struct ExecutionOptions` extended with `budget: Option<u64>`.
  - `Command::parse` configured to accept `--budget=<N>`, `--budget <N>`,
    `--max-instructions=<N>` and `--max-instructions <N>`, rejecting use outside `run`,
    invalid values and duplicate flags.
  - Budget propagated identically to:
    1. **WebAssembly JIT:** forwarded to `aipo_wasm::execute_wasm_with_fuel(&bytes, out, budget)`.
    2. **Register VM (`--engine=reg`):** forwarded by calling `reg_vm.set_max_instructions(budget)`.
    3. **Stack VM (default):** forwarded by calling `vm.set_max_instructions(budget)`.
    4. **Pre-compiled bytecode (`.aibc`):** forwarded by calling `vm.set_max_instructions(budget)`.
- In `crates/aipo-cli/tests/wasm_cli.rs`:
  - Added `test_cli_instruction_budget_halts_infinite_loops`: validates interruption of infinite
    loops and emission of `AIPO_RT_OVERFLOW` on all three engines (Wasm JIT, Stack VM and Reg VM).
  - Added `test_cli_budget_happy_path_completes`: validates completion with exit code 0 when the
    budget is sufficient.
  - Added `test_cli_budget_flag_validation`: validates error messages for invalid, duplicate or
    unsupported-command flags.

---

## 3. Quality and conformance evidence

- **Wasm tests:** 101 passing (0 failures) in `crates/aipo-wasm`.
- **VM tests:** 154 passing (0 failures) in `crates/aipo-vm`.
- **CLI conformance tests:** 23 passing in `crates/aipo-cli --test conformance`.
- **CLI shell tests:** 13 passing in `crates/aipo-cli --test shell_tests`.
- **CLI Wasm & budget tests:** 11 passing in `crates/aipo-cli --test wasm_cli`.
- **Clippy and formatting:** 100% green with `-D warnings` and `cargo fmt --all --check`.
