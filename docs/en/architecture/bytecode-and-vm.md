# Bytecode & Virtual Machine

Aipo executes natively on a **deterministic bytecode virtual machine**, optimized for low memory footprint and high-throughput instruction dispatch.

---

## The `.aibc` Binary Format

The Aipo compiler can serialize bytecode chunks directly into an `.aibc` binary archive:

- **Magic Header**: 4-byte signature `AIBC` followed by format version integer (currently `1`).
- **Constant Pool**: Integer literals, floating-point numbers, normalized strings, and function prototypes.
- **Debug Metadata Table**: Bytecode instruction offsets mapped to source code lines and columns.
- **Code Section**: Compact binary encoding of virtual machine instructions.

---

## Virtual Machine Architecture (`aipo-vm`)

### 1. Value Representation (`Value`)
`Value` layout is target-specific and measured with `cargo run -p aipo-vm --example layout_probe`.
On this x86-64 workspace it is **16 bytes**; there is no portable fixed-size promise.
- Primitives: `Int(i64)`, `Float(f64)`, `Bool(bool)`, `None`
- Reference-counted heap variants: `String(Rc<str>)`, `List(Rc<RefCell<Collection<Value>>>)`, `Dict`, `Set(Rc<RefCell<Collection<Value>>>)`, `Bytes(Rc<RefCell<Collection<u8>>>)`
- Execution entities: `Closure`, `StructMethod`, `BoundMethod`, `Task`, `HostHandle`

### 2. Dispatch Loop Optimization
- Instructions are decoded directly via flattened `match` branches, eliminating bulky 72-byte `Result` allocations on every CPU cycle.
- Inline monomorphic caches for struct field resolution, accelerating property lookups by up to 28%.
- Formal bytecode verifier asserting local slot bounds, upvalues, and frame depth limits prior to execution.

### 3. Integrated Disassembler
The `aipo disasm` command disassembles bytecode instructions and maps each mnemonic directly to its originating source line, streamlining profiling, debugging, and security audits.
