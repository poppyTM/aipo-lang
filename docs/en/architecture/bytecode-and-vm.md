# Bytecode & virtual machine

Native execution of Aipo runs on a **deterministic bytecode virtual machine**, optimized for low
memory consumption and fast instruction dispatch.

---

## The `.aibc` binary format

The Aipo compiler can serialize bytecode directly to a binary `.aibc` file:

- **Magic header**: 4 bytes `AIBC` followed by the format version (currently `1`).
- **Constant table**: integer and decimal literals, normalized strings and function references.
- **Debug metadata table**: mapping from bytecode offsets to lines and columns in the original
  source.
- **Code section**: linear VM instructions in compact binary form.

---

## Virtual machine architecture (`aipo-vm`)

### 1. Value model (`Value`)
The `Value` layout is measured per target with
`cargo run -p aipo-vm --example layout_probe`. On this x86-64 workspace it occupies **16
bytes**; there is no portable promise of a fixed size.
- Primitives: `Int(i64)`, `Float(f64)`, `Bool(bool)`, `None`
- Reference-counted heap: `String(Rc<str>)`, `List(Rc<RefCell<Collection<Value>>>)`, `Dict`,
  `Set(Rc<RefCell<Collection<Value>>>)`, `Bytes(Rc<RefCell<Collection<u8>>>)`
- Execution objects: `Closure`, `StructMethod`, `BoundMethod`, `Task`, `HostHandle`

### 2. Dispatch-loop optimization
- Instructions decoded directly through a simple `match`, removing the cost of the 72-byte
  `Result` on every CPU cycle.
- Monomorphic caches for field-slot resolution on struct instances, reducing property-access
  time by up to 28%.
- Formal bytecode verifier that validates local-slot bounds, upvalues and frame sizes before
  execution starts.

### 3. Integrated disassembler
The `aipo disasm` tool decomposes bytecode and relates each mnemonic to the exact source line
that originated it, easing profiling and security audits.
