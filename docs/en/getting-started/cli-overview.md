# Command-Line Interface Guide (`aipo`)

The `aipo` executable is the unified toolchain utility for execution, static checking, JavaScript code generation, bytecode disassembly, code formatting, and hermetic package auditing.

---

## Primary Subcommands

### `aipo run`

Compiles and executes an Aipo source file (`.aipo`) or precompiled bytecode (`.aibc`):

```bash
# Run source file directly
aipo run src/main.aipo

# Run with custom package cache directory
aipo run src/main.aipo --package-cache .aipo/cache

# Structured JSONL diagnostic output (ideal for IDEs and CI pipelines)
aipo run src/main.aipo --message-format=jsonl
```

### `aipo test`

Discovers and executes unit tests in `*_test.aipo`, `test_*.aipo`, or files with a `#!test` directive, with isolation, zeroed PRNG seeds, and a frozen virtual clock. Without an argument it scans `tests/` and the current directory; pass a package directory to run a package:

```bash
# Run a package's tests
aipo test packages/aipo-ui

# Filter tests by name or path pattern
aipo test packages/aipo-ui --filter layout

# Streaming JSONL output for CI/CD
aipo test packages/aipo-ui --message-format=jsonl
```

### `aipo check`

Runs the complete static analysis pipeline (Lexer, Parser, HIR, SEMA, and bytecode verification) without running the VM:

```bash
aipo check src/main.aipo
```

Accurately reports syntax errors, incompatible interface contracts, unbound variable references, and illegal mutations of immutable fields.

### `aipo build`

Emits the complete JavaScript bundle (`app.js`, `aipo-runtime.js`, and `app.js.map` sourcemap) ready for deployment in modern browsers or Node.js runtimes:

```bash
# Compile and emit bundle into output directory
aipo build src/main.aipo --out dist/
```

### `aipo disasm`

Disassembles an `.aipo` or `.aibc` file, displaying VM instructions, byte offsets, constant pool entries, and mapped source coordinates:

```bash
aipo disasm src/main.aipo
```

### `aipo fmt`

Formats Aipo source files according to canonical language style conventions:

```bash
# Format files in place
aipo fmt src/main.aipo

# Verify formatting without writing changes (CI mode)
aipo fmt --check src/
```

### `aipo package`

Hermetic commands for package management, lockfile generation, and security auditing:

```bash
# Create or update deterministic lockfile (aipo.lock)
aipo package lock .

# Create lockfile fetching remote GitHub snapshots into local cache
aipo package lock . --fetch-github --cache .aipo/cache

# Audit integrity and manifest compliance of a local package
aipo package audit .

# Verify cryptographic SHA-256 integrity across all cached entries
aipo package cache verify .aipo/cache

# Prune unreferenced, stale cache entries safely
aipo package cache prune .aipo/cache --lock aipo.lock --apply
```

::: tip 📖 Comprehensive Tooling Guide
For in-depth details on compiler flags, WebAssembly targets, CI/CD integrations, and real-world examples, visit the [Complete Developer Tooling Guide](/en/tools/).
:::
