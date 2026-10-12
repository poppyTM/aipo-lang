# aipo-vm integration tests

Integration test suites for the Aipo virtual machine interpreter and embedding engine.

## Test Suites

- **`engine_scope_tests.rs`**: Safe Rust embedding engine (`AipoEngine`), builder pattern, execution scopes (`AipoScope`), lifetime-scoped handles (`ScopedHandle`), generational userdata, and scope escape dynamic verification.
- **`host_capability_and_handles.rs`**: Host ABI capability verification and generational handle table integration.
- **`host_scope_escape.rs`**: Verification of scope boundary enforcement and publication checking for host handles.
- **`vm_core.rs`**: Opcode execution, call frames, arithmetic, instruction budget enforcement, and native function dispatch.
- **`data_and_errors.rs`**: Lists, dicts, structs, invariants, and Failure Model B recovery semantics.
- **`contracts_matrix.rs`**: Matrix testing of parameter contracts, return contracts, and interface conformance.
- **`reg_window_tests.rs`**: Register windowing on `RegVm` (AIPO-OMNI-005), asymmetric caller/callee activation frames, recursive Fibonacci stacks, runtime fault recovery, and cooperative task suspension.
- **`interval_metering_tests.rs`**: Periodic instruction budget metering (AIPO-OMNI-004), amortized dispatch fuel checks on Stack VM and RegVm, fine-grained single step, and return reconciliation.
- **`cycle_guard_tests.rs`**: Prevention and detection of reference cycles during collection mutation.
- **`pipeline_integration.rs`**: End-to-end integration testing from parsing to bytecode execution.
- **`value_properties.rs`**: Value size bounds, numeric limits, structural equality, and float finiteness invariants.
- **`wave3_async_semantics.rs`**: Async tasks, cancellation, join sets, and task timeouts.
- **`wave3_pipeline.rs`**: Async combinators, duration arithmetic, and set operations.
