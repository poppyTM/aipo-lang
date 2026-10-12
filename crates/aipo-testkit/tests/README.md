# aipo-testkit integration tests

Integration test suites for memory lifecycle, leak detection, and pipeline verification.

## Test Suites

- **`leak_canaries.rs`**: Exercises a `CountingAllocator` monitoring atomic live bytes across sequential execution churn (warmup + 32 iterations) across 8 distinct lifecycle scenarios:
  1. `vm_churn`: Stack VM lifecycle and collection allocation/deallocation.
  2. `closure_churn`: Higher-order closure capture and upvalue cell collection.
  3. `reg_vm_churn`: Register VM module compilation, execution, and drop.
  4. `frontend_churn`: Lexer, parser, HIR lowering, and semantic checker lifecycle.
  5. `compiler_churn`: Full source-to-verified bytecode module compilation.
  6. `wasm_compile_churn`: HIR lowering, WebAssembly binary compilation, and WAT disassembly.
  7. `handle_table_churn`: Generational handle table allocation and clearing.
  8. `engine_scope_churn`: Scoped engine (`AipoEngine::scope`), userdata insertion, native callbacks, and RAII teardown.
