# Developer Tooling (`aipo` CLI)

The development experience in Aipo is designed to be complete and battery-included out of the box. Unlike fragmented ecosystems that require multiple external tools for formatting, testing, and bundling, **Aipo delivers a unified toolchain inside a single fast binary (`aipo`)**.

<div class="tool-grid">

<div class="tool-card">
  <div>
    <h3>▶️ aipo run</h3>
    <p>Executes source files directly, precompiled <code>.aibc</code> bytecode, or WebAssembly modules with a cooperative async scheduler.</p>
  </div>
  <div class="tool-cmd">aipo run src/main.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>🧪 aipo test</h3>
    <p>Discovers and runs unit tests automatically with strict test isolation, zeroed PRNG seeds, and deterministic frozen time.</p>
  </div>
  <div class="tool-cmd">aipo test --filter math</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔍 aipo check</h3>
    <p>Instant static verification: validates syntax, mutability constraints, lexical scope, and interface contracts prior to execution.</p>
  </div>
  <div class="tool-cmd">aipo check src/main.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>✨ aipo fmt</h3>
    <p>Canonical zero-configuration code formatter with comment preservation and CI drift checking mode.</p>
  </div>
  <div class="tool-cmd">aipo fmt --check src/</div>
</div>

<div class="tool-card">
  <div>
    <h3>📦 aipo build</h3>
    <p>Emits optimized ES2022 JavaScript bundles with source maps or WebAssembly binaries ready for browsers or Node.js.</p>
  </div>
  <div class="tool-cmd">aipo build src/main.aipo --out dist/</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔒 aipo package</h3>
    <p>Hermetic package manager: deterministic lockfiles, commit-SHA pinned GitHub dependencies, and immutable local cache.</p>
  </div>
  <div class="tool-cmd">aipo package audit .</div>
</div>

</div>

---

## 1. Execution: `aipo run`

The `aipo run` subcommand compiles and runs Aipo programs with the high-performance native runtime.

```bash
# Execute source script directly
aipo run src/main.aipo

# Execute with explicit package cache directory
aipo run src/main.aipo --package-cache .aipo/cache

# Diagnostic output in structured JSONL format (ideal for IDEs and CI)
aipo run src/main.aipo --message-format=jsonl

# Execute precompiled bytecode (sub-millisecond instant startup)
aipo run bin/app.aibc
```

### Exit Codes

Aipo enforces deterministic exit codes:

| Code | Meaning | Description |
| :---: | :--- | :--- |
| `0` | **Success** | The program executed to completion with no uncaught failures. |
| `1` | **Language Failure** | Syntax error, semantic error, runtime failure (`fail(...)`), or formatting drift. |
| `2` | **Usage Error** | Invalid command-line arguments, unknown flags, or nonexistent files. |

---

## 2. Automated Testing: `aipo test`

The `aipo test` command is the built-in test runner. Without an argument it scans `tests/` and the current directory; pass a package directory to run a package. It discovers files ending in `_test.aipo`, starting with `test_*.aipo`, or containing a `#!test` directive. Suites that call `host_*` need the host bridge.

```bash
# Run a package's tests
aipo test packages/aipo-ui

# Filter tests by function name or path pattern
aipo test packages/aipo-ui --filter layout

# Machine-readable output for CI/CD integrations
aipo test packages/aipo-ui --message-format=jsonl
```

### Determinism and Isolation

Every test runs in total isolation:
- **Clean VM**: Every test function executes against a fresh Virtual Machine instance.
- **Frozen Clock**: Virtual time is initialized to `0` and real system time never ticks spontaneously, eliminating flaky tests.
- **Predictable PRNG**: The pseudo-random number generator seed is reset to `0` before each test.

```text
# math_test.aipo
fn test_addition() {
    let result = 2 + 2
    if result != 4 {
        fail("expected 4 from addition")
    }
}

fn test_integer_division() {
    let value = 14 // 3
    if value != 4 {
        fail(f"expected 4 from truncated division, got {value}")
    }
}
```

---

## 3. Static Analysis: `aipo check`

The `aipo check` command executes the full compiler frontend (Lexer, Parser, HIR Lowering, Semantic Analysis, and Bytecode Verification) **without running the program**:

```bash
aipo check src/main.aipo
```

It validates:
- Syntax and proper `{ ... }` block balancing.
- Identifier resolution and scope visibility.
- Immutability violations (attempting to mutate fields not declared with `var`).
- Static interface contract compliance and function call arity.

---

## 4. Code Formatting: `aipo fmt`

Aipo includes an opinionated, canonical code formatter:

```bash
# Format one or multiple files in-place
aipo fmt src/main.aipo

# Format an entire source directory recursively
aipo fmt src/

# CI mode: checks whether all files match canonical formatting without modifying them
aipo fmt --check src/
```

### Canonical Rules:
- Standard **4 spaces** indentation (no tabs).
- Symmetrical `{ ... }` blocks aligned with opening statements.
- Readable inner spacing for single-line structures: `User{ id: 1, name: "Ana" }`.
- Preserves code comments and intentional developer blank lines.

---

## 5. Web & JavaScript Bundling: `aipo build`

The Aipo compiler produces optimized outputs for Web and Node.js with **100% bit-for-bit differential parity**:

```bash
# Compile to ES2022 JavaScript bundle
aipo build src/main.aipo --out dist/

# Compile to native WebAssembly
aipo build src/main.aipo --target wasm --out dist/
```

The emitted bundle includes:
- `app.js`: Application logic and entry point.
- `aipo-runtime.js`: Modular, versioned runtime shim with zero external dependencies.
- `app.js.map`: Complete source map for direct debugging of `.aipo` source in browser DevTools.
- `package.json`: Declares `"type": "module"` so `node app.js` works in the output directory.

---

## 6. Hermetic Package Management: `aipo package`

Aipo uses deterministic, reproducible package management driven by `aipo.toml` and `aipo.lock`.

```bash
# Lock local dependencies and generate aipo.lock
aipo package lock .

# Fetch remote GitHub dependencies and store them in the cache
aipo package lock . --fetch-github --cache .aipo/cache

# Optional authentication for private repositories
aipo package lock . --fetch-github --cache .aipo/cache --github-token-env GITHUB_TOKEN

# Audit package structure and manifest integrity
aipo package audit .

# Verify SHA-256 digests of all cache entries
aipo package cache verify .aipo/cache

# Prune unreferenced cache entries safely
aipo package cache prune .aipo/cache --lock aipo.lock --apply
```

---

## 7. Disassembler: `aipo disasm`

For compiler engineers, researchers, and performance optimization, `aipo disasm` inspects compiled opcodes:

```bash
aipo disasm src/main.aipo
```

It prints VM instruction mnemonics, byte offsets, constant pool references, and mapped source coordinates.

---

## Continuous Integration Recipe (GitHub Actions)

Add this workflow to `.github/workflows/ci.yml` for automated verification of your Aipo code:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust Toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Build Aipo CLI
        run: cargo build --release -p aipo-cli

      - name: Add aipo to PATH
        run: echo "$(pwd)/target/release" >> $GITHUB_PATH

      - name: Check Formatting
        run: aipo fmt --check examples/

      - name: Static Check
        run: aipo check examples/01_fizzbuzz.aipo

      - name: Run Unit Tests
        run: aipo test packages/aipo-ui
```
