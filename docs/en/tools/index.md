# Developer tools (CLI `aipo`)

The Aipo development experience is designed to be complete and immediate. Unlike fragmented
ecosystems that require multiple external packages for formatting, testing and packaging, **Aipo
delivers a full native tool suite in a single binary (`aipo`)**.

<div class="tool-grid">

<div class="tool-card">
  <div>
    <h3>▶️ aipo run</h3>
    <p>Runs source files directly, pre-compiled `.aibc` bytecodes or WebAssembly modules with the
    cooperative async scheduler.</p>
  </div>
  <div class="tool-cmd">aipo run src/main.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>🧪 aipo test</h3>
    <p>Discovers and runs unit tests automatically with strict isolation between tests, a zeroed
    PRNG seed and deterministic frozen time.</p>
  </div>
  <div class="tool-cmd">aipo test --filter math</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔍 aipo check</h3>
    <p>Instant static analysis: validates syntax, mutability rules, scope and interface contracts
    before execution.</p>
  </div>
  <div class="tool-cmd">aipo check src/main.aipo</div>
</div>

<div class="tool-card">
  <div>
    <h3>✨ aipo fmt</h3>
    <p>Opinionated canonical formatter with comment preservation, no configuration required and
    CI audit mode support.</p>
  </div>
  <div class="tool-cmd">aipo fmt --check src/</div>
</div>

<div class="tool-card">
  <div>
    <h3>📦 aipo build</h3>
    <p>Emits optimized JavaScript ES2022 bundles with source maps or WebAssembly binaries to run
    in the browser or Node.js.</p>
  </div>
  <div class="tool-cmd">aipo build src/main.aipo --out dist/</div>
</div>

<div class="tool-card">
  <div>
    <h3>🔒 aipo package</h3>
    <p>Hermetic package manager: deterministic lockfiles, GitHub dependencies pinned by commit
    SHA and an immutable local cache.</p>
  </div>
  <div class="tool-cmd">aipo package audit .</div>
</div>

</div>

---

## 1. Execution: `aipo run`

The `aipo run` subcommand compiles and runs Aipo code with the native high-performance runtime.

```bash
# Direct execution of a script
aipo run src/main.aipo

# Execution with an explicit package cache directory
aipo run src/main.aipo --package-cache .aipo/cache

# Diagnostics in JSONL format (ideal for editors and IDEs)
aipo run src/main.aipo --message-format=jsonl

# Execution of pre-compiled bytecode (ultra-fast startup without compilation)
aipo run bin/app.aibc
```

### Exit codes

Aipo adopts deterministic, standardized exit codes:

| Code | Meaning | Description |
| :---: | :--- | :--- |
| `0` | **Success** | The program ran to completion without uncaught failures. |
| `1` | **Language failure** | Semantic error, syntax error, runtime failure (`fail(...)`) or formatting deviation. |
| `2` | **Usage error** | Invalid command-line arguments, unknown flags or missing files. |

---

## 2. Automated testing: `aipo test`

The `aipo test` command is the language's native test runner. With no argument it scans `tests/`
and the current directory; for a package, pass the directory. It recognizes `*_test.aipo` files,
`test_*.aipo` files and files carrying the `#!test` directive. Suites that call `host_*` need
the host bridge.

```bash
# Run a package's tests
aipo test packages/aipo-ui

# Filter tests by name or path
aipo test packages/aipo-ui --filter layout

# Machine-readable output for CI/CD integration
aipo test packages/aipo-ui --message-format=jsonl
```

### Isolation and determinism in tests

Each test runs in an environment of absolute isolation:
- **Clean VM**: every test gets a completely fresh Virtual Machine instance.
- **Frozen clock**: virtual time starts at `0` and the real clock does not advance on its own,
  eliminating flaky tests.
- **Predictable PRNG**: the random seed is reset to `0` before each test case.

```text
# math_test.aipo
fn test_soma_simples() {
    let resultado = 2 + 2
    if resultado != 4 {
        fail("esperava 4 na soma")
    }
}

fn test_divisao_inteira() {
    let valor = 14 // 3
    if valor != 4 {
        fail(f"esperava 4 na divisão truncada, obteve {valor}")
    }
}
```

---

## 3. Static analysis: `aipo check`

The `aipo check` command runs the entire compiler pipeline (Lexer, Parser, HIR, Semantic
Analysis and Bytecode Verification) **without starting execution**:

```bash
aipo check src/main.aipo
```

It validates:
- Syntax and correct closing of `{ ... }` blocks.
- Resolution and visibility of identifiers and variables.
- Immutability violations (attempts to mutate fields not declared with `var`).
- Static conformance of interface contracts and call arities.

---

## 4. Code formatting: `aipo fmt`

Aipo ships an opinionated canonical formatter with well-defined rules:

```bash
# Format one or more files in place
aipo fmt src/main.aipo

# Format every file in the project recursively
aipo fmt src/

# CI mode: only checks whether code is formatted (exit code 1 if there are deviations)
aipo fmt --check src/
```

### Canonical format rules:
- Default indentation of **4 spaces** (no tabs).
- `{ ... }` blocks aligned with the closing brace on the same level as the opening command.
- Readable internal spacing in single-line literals: `User{ id: 1, name: "Ana" }`.
- Intact preservation of comments and intentional line breaks.

---

## 5. Web and JavaScript emission: `aipo build`

The Aipo compiler produces optimized code for the Web and Node.js ecosystem, with
differential parity proven on the corpus:

```bash
# Compile project to JavaScript ES2022
aipo build src/main.aipo --out dist/

# Compile to native WebAssembly
aipo build src/main.aipo --target wasm --out dist/
```

The generated bundle includes:
- `app.js`: application code and runtime bootstrap.
- `aipo-runtime.js`: modular, versioned shim with no external dependencies.
- `app.js.map`: full source map for debugging `.aipo` files directly in the browser DevTools.
- `package.json`: declares `"type": "module"` so `node app.js` works in the output directory.

---

## 6. Hermetic package management: `aipo package`

The Aipo ecosystem adopts deterministic, reproducible dependencies based on `aipo.toml` and
`aipo.lock`.

```bash
# Pin local dependencies and generate the deterministic lockfile
aipo package lock .

# Fetch remote dependencies from GitHub and store them in the cache
aipo package lock . --fetch-github --cache .aipo/cache

# Optional authentication for private repositories
aipo package lock . --fetch-github --cache .aipo/cache --github-token-env GITHUB_TOKEN

# Audit integrity and conformance of dependencies
aipo package audit .

# Verify SHA-256 hashes of every entry in the local cache
aipo package cache verify .aipo/cache

# Remove old and orphaned versions from the local cache
aipo package cache prune .aipo/cache --lock aipo.lock --apply
```

---

## 7. Disassembler: `aipo disasm`

For enthusiasts, compiler researchers and performance engineers, `aipo disasm` exposes the
opcodes and internal structure that were generated:

```bash
aipo disasm src/main.aipo
```

Displays VM mnemonics, constant references, instruction sizes and the exact line/column
mapping in the original source code.

---

## CI/CD workflow example (GitHub Actions)

Add this workflow to `.github/workflows/ci.yml` to ensure continuous quality in your Aipo
repository:

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

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Build Aipo CLI
        run: cargo build --release -p aipo-cli

      - name: Add aipo to PATH
        run: echo "$(pwd)/target/release" >> $GITHUB_PATH

      - name: Check formatting
        run: aipo fmt --check examples/

      - name: Static check
        run: aipo check examples/01_fizzbuzz.aipo

      - name: Run unit tests
        run: aipo test packages/aipo-ui
```
