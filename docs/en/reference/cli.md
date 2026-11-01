# Aipo — CLI reference

**Status:** normative
**Scope:** command surface, machine output, exit codes and examples for the Aipo CLI
**Blocking question:** Which commands are stable?

## Stable commands

The stable command surface comprises (`run`/`check`/`fmt` stable since Wave 1,
`build` stable since Wave 2, `disasm` stable since Wave 4, `test` and Wasm targets stable since
v0.1.0/ADP-013):

- `aipo run <file.aipo> [--wasm] [--engine=<vm|reg>] [--budget=<N>] [--max-instructions=<N>] [--package-cache <dir>] [--ahs=<file>] [--host=headless-test]`: Compiles and executes an Aipo source file via Stack VM (default), Register VM (`--engine=reg`), or WebAssembly JIT engine (`--wasm` / `-t wasm`). Execution budgets (`--budget=<N>` / `--max-instructions=<N>`) bound instruction or fuel consumption. `--ahs` makes the described host surface available to checking; it does not install host implementations or grant capabilities.
- `aipo run <file.aibc|file.wasm> [--budget=<N>] [--max-instructions=<N>]`: Loads, verifies and executes a pre-compiled bytecode file or WebAssembly binary with optional budget metering.
- `aipo test [path] [--filter <pattern>] [--package-cache <dir>]`: Discovers and executes isolated unit tests (`*_test.aipo`, `test_*.aipo`, or files containing `#!test`) with temporal freezing and PRNG seed reset. Without `path`, it scans `tests/` and files in the current directory; pass a package directory for a full suite. Suites that call `host_*` need the host bridge (`aipo-game-host/tests/bridge_tests.rs`) rather than the standalone runner.
- `aipo check <file.aipo> [--wasm] [--package-cache <dir>] [--ahs=<file>] [--host=headless-test]`: Runs the frontend and semantic analysis without execution, emitting diagnostics. `--wasm` additionally reports `AIPO_COMPILE_WASM_UNSUPPORTED` when the current backend cannot represent a construct. `--ahs` loads a validated Aipo Host Schema (AHS) JSON description for this invocation; described host modules and callable signatures (unknown member, arity with optional parameters, named arguments, literal contract violations) are available to static checking.
- `aipo build <file.aipo> [--target <js|wasm>] [--out <dir>] [--package-cache <dir>]`: Emits an ESM JavaScript bundle (`app.js`, `aipo-runtime.js`, source map and `package.json` with `"type": "module"`) or a standalone WebAssembly binary (`app.wasm`). Without `--out`, artifacts go to `<entry-dir>/dist`.
- `aipo disasm <file.aipo|file.aibc|file.wasm> [--wasm] [--package-cache <dir>]`: Disassembles a source, bytecode, or Wasm file (printing bytecode disassembly or WebAssembly Text / WAT format).
- `aipo fmt [files...] [--check]`: Formats source files idempotently according to canonical indentation rules.
- `aipo package lock <package-dir> [--fetch-github --cache <dir>] [--github-token-env <name>]`: Resolves a local package graph and writes `aipo.lock` in the package root.
- `aipo package audit <package-dir>`: Resolves a local package graph and verifies the existing `aipo.lock` without rewriting it.
- `aipo package cache verify <cache-dir>`: Audits every existing GitHub cache entry without network access or filesystem mutation.
- `aipo package cache prune <cache-dir> --lock <lockfile> [--apply]`: Removes only verified cache entries not referenced by the explicit lockfile; dry-run is the default.

## Experimental register backend

`aipo run <file.aipo> --engine=reg` selects an explicit plan: native Reg when the emitter and
verifier accept the IR, or canonical VM for capabilities still absent. `aipo plan <file.aipo>`
compiles without running and prints JSON with the real engine and reason.
`analyze_to_reg_module` stays strict and reports `AIPO_COMPILE_REG_UNSUPPORTED`; the plan API
represents the fallback. On 2026-10-27, 20/36 conformance programs ran natively in Reg;
higher-order methods, failure handlers and non-representable IR use the VM. Hosts that depend
on VM context use the canonical plan.

An unhandled recoverable failure at the top is reported as `AIPO_RT_FAILURE_UNCAUGHT` with exit
`1`. Arithmetic uses the shared Value helpers; `each key in dict` projects keys in insertion
order.

New budgets are Rust APIs: `RegVm::set_instruction_budget` and
`execute_wasm_with_options`/`WasmExecutionOptions`. There is no new CLI flag for fuel in this
revision. `wasm` enables the compiler; `wasmtime-runner` enables the JIT.

See the [complete guide](../development/runtime-hardening-guide.md) for limits, integration,
migration and pending validation.

## Host surface descriptions

`run` and `check` accept `--ahs=<file>` or `--ahs <file>` for Aipo source inputs.
The JSON description is validated before compilation. Invalid JSON, inconsistent schemas,
missing files, empty paths and repeated flags are usage errors (exit `2`). The flag is not
accepted by other commands or for precompiled `.aibc`/`.wasm` inputs.

The surface is passed explicitly to this compilation: no process-global host schema is
installed, so subsequent invocations and embedding compilations do not inherit it. Direct
host-module calls are checked for unknown members, required/optional parameters, named
arguments and literal parameter-contract violations. Local bindings that shadow a module are
checked as local values, not against the host signature.

The same semantic checks run before Wasm compilation, but an AHS does not add Wasm imports
or runtime implementations. Calls unsupported by the selected backend can still fail after
semantic checking. `run --ahs` also does not grant capabilities or install VM natives.

Rust embedders can call `aipo_cli::load_host_surface(path)` and pass the returned
`PreludeSurface` to `compile_file(path, Some(&surface))` or `analyze_with_surface`.

## Headless conformance host

`aipo run file.aipo --host=headless-test` explicitly installs an in-memory conformance host.
`check` accepts the same flag (or `--host headless-test`) to check its embedded AHS without
executing anything. It is VM-only and rejects Wasm targets, precompiled inputs, other
commands, unknown/repeated profiles, and combinations with `--ahs` (exit `2`). The profile
supplies its own AHS; an arbitrary description never selects an implementation.

The profile grants no capabilities, installs no filesystem/environment provider, and uses only
VM-local state. `time.now()` and `time.monotonic()` fault with `AIPO_RT_CAPABILITY_DENIED`
without revoking the ordinary CLI's clock. The following one-argument functions form a
deliberately small testing surface:

| Function | Behavior |
|---|---|
| `headless.create(value: Int)` | Create an unscoped handle holding a copied integer. |
| `headless.read(handle)` | Read the integer; a released handle faults with `AIPO_RT_STALE_HANDLE`. |
| `headless.release(handle)` | Release a live handle; its generation is never reused. |
| `headless.scoped(value: Int)` | Intentionally return a handle after its native scope closes, so the next heap publication faults with `AIPO_RT_SCOPE_ESCAPE`. |

`scoped` is a negative-test probe, not a general callback API. The fault comes from the real
VM publication checks (global, return, field, index, list or dict), not a fabricated error.
Runtime argument contracts also apply to dynamic values. Faults are not recoverable `Failure`
values. Fixtures in `docs/conformance/host/` pair `.aipo` with `.code` or `.stdout` and are run
through the real CLI by `crates/aipo-cli/tests/host_runtime.rs`.

## Execution engines and metering

`aipo run` supports selectable bytecode execution engines for `.aipo` source files:
- `--engine=vm` (default): Stack-based Virtual Machine with direct operand evaluation,
  cooperative task scheduling, and full host/stdlib integration.
- `--engine=reg`: Register-based Virtual Machine (`crates/aipo-vm/src/reg_vm.rs`) with
  windowed register execution. Cannot be combined with `--wasm` and requires `.aipo` source.
- `--wasm`: Standalone WebAssembly JIT engine powered by `wasmtime`.

### Instruction and fuel budget

To protect against runaway loops and enforce deterministic bounded execution, `aipo run`
accepts `--budget=<N>` (or alias `--max-instructions=<N>`):
- For Stack VM and Register VM, execution faults if the executed instruction count exceeds `N`.
- For WebAssembly, `wasmtime` fuel metering consumes fuel up to `N`.

## Local package entries

For `.aipo` inputs, `run`, `check`, `build`, and `disasm` look for the nearest `aipo.toml` in
the entry file's ancestor directories. When present, local `path` dependencies are resolved
before module linking. A qualified import such as `import acme.http` binds the local namespace
`http`; `import acme.http as h` binds `h`. The full coordinate remains the module identity.

Resolution for these source commands is local-only unless `--package-cache <dir>` is explicitly
provided. The flag consumes a previously locked mixed local/GitHub graph and its verified cache
entries without network access. A missing lockfile is allowed only without the flag. Without an
`aipo.toml`, source files keep the legacy sibling-module lookup.

## Opt-in public GitHub fetch

The HTTP adapter is compiled only with the `github-http` feature. The default `aipo` binary
remains network-free. Enable it explicitly when fetching a public package or locking a mixed
root:

```bash
cargo run -p aipo-cli --features github-http -- \\
  package fetch-github owner/repository 0123456789abcdef0123456789abcdef01234567 \\
  [subpath] --cache <cache-dir> --out <output-dir> [--github-token-env <name>]

cargo run -p aipo-cli --features github-http -- \\
  package lock ./my-package --fetch-github --cache <cache-dir> \\
  [--github-token-env <name>]
```

The command accepts only a public repository slug, an exact lowercase 40-hex commit, and a safe
relative subpath. A manifest dependency uses the same source fields and also requires an exact
SemVer version:

```toml
[dependencies]
"acme.http" = { version = "1.0.0", type = "github", repository = "acme/packages", revision = "0123456789abcdef0123456789abcdef01234567", subpath = "." }
```

By default, the command sends public GET requests without `Authorization` or other credentials.
With `--github-token-env <name>`, the named environment variable supplies a validated bearer
credential. The variable value is never accepted as a CLI value, persisted, logged or included
in diagnostics. Missing, empty or control-character-bearing values fail before the request.
Requests always use HTTPS, reject redirects, apply a timeout and a 4 MiB response limit, and use
the explicit cache root. The command recursively fetches pinned GitHub dependencies, validates
coordinates, versions, capabilities and cycles, enforces a 256-package graph bound, and writes a
complete graph lockfile in the output snapshot. Each artifact is cached and digest-verified
independently. The output directory must be empty. A remote artifact cannot declare a local
`path` dependency in this slice.

For a mixed local root, `package lock --fetch-github` leaves local path inputs in the project
and fetches only its pinned GitHub edges into the same explicit cache. This is the only lock
form that can access the network; plain `package lock` and `package audit` remain local-only.

## Offline cache consumption

A locked mixed local/GitHub graph can be compiled without the HTTP feature or network access:

```bash
aipo check my-package/src/main.aipo --package-cache .aipo-cache
aipo run my-package/src/main.aipo --package-cache=.aipo-cache
```

The command requires `aipo.lock`, verifies every cached GitHub manifest and entry against the
lock projection, follows all local path branches, and never creates a missing cache root. A
missing artifact, corrupt cache, or stale lock fails closed. The cache is read-only for these
commands; only `fetch-github` and `package lock --fetch-github` write it.

Failures use `AIPO_PKG_FETCH` for transport/cache/artifact failures and `AIPO_PKG_RESOLUTION`
for graph failures. Without `--package-cache`, source commands never touch the cache or network.
Plain `package lock` and `package audit` remain local-only; only the explicit mixed-lock form can
fetch.

## Cache verification

Cache verification is an explicit, read-only maintenance operation:

```bash
aipo package cache verify .aipo-cache
```

The verifier scans namespace entries in deterministic order and checks directory/file safety,
metadata source identity, cache-key consistency, manifest parsing and SHA-256 content digests.
It continues after independent entry failures and reports each failure as `AIPO_PKG_FETCH`.
A missing or empty cache is a successful zero-entry report. Verification never creates,
repairs, deletes, prunes or fetches cache data and is available in the default network-free
build.

## Cache pruning and retention

Pruning is lockfile-driven and deliberately conservative:

```bash
aipo package cache prune .aipo-cache --lock aipo.lock
aipo package cache prune .aipo-cache --lock aipo.lock --apply
```

The first form is a dry-run and lists verified entries not referenced by the lockfile. `--apply`
is required for deletion. If verification finds any invalid entry, pruning fails closed and
deletes nothing; corrupt or unknown entries are never removed automatically. Sources referenced
by the lockfile are always retained, and the lockfile itself is never changed. Pruning performs
no network access, authentication or registry lookup.

## Command surface

```text
aipo run <path> [--wasm] [--package-cache <dir>] [--ahs=<file>] [--host=headless-test] [--message-format=<human|jsonl>]
aipo test [path] [--filter <pattern>] [--package-cache <dir>] [--message-format=<human|jsonl>]
aipo check <path> [--wasm] [--package-cache <dir>] [--ahs=<file>] [--host=headless-test] [--message-format=<human|jsonl>]
aipo build <path> [--target <js|wasm>] [--out <dir>] [--package-cache <dir>] [--message-format=<human|jsonl>]
aipo disasm <path> [--wasm] [--package-cache <dir>] [--message-format=<human|jsonl>]
aipo fmt <paths...> [--check]
aipo package lock <package-dir> [--fetch-github --cache <dir>] [--github-token-env <name>]
aipo package audit <package-dir>
aipo package cache verify <cache-dir>
aipo package cache prune <cache-dir> --lock <lockfile> [--apply]
aipo --version
aipo --help
```

## Machine output

When `--message-format=jsonl` is specified, all compiler and package errors, warnings, and runtime
faults are emitted to standard error as newline-delimited JSON matching the diagnostic catalog
schema:

```json
{"code":"AIPO_SEM_UNKNOWN_NAME","severity":"error","message":"unknown variable 'y'","primary_span":{"file":"main.aipo","start":11,"end":16,"line":2,"column":1},"notes":[],"suggestions":[]}
```

## Exit codes

- `0`: Success (program ran to completion, check found no errors, or format succeeded).
- `1`: Language/package failure (parse error, semantic error, package resolution or stale-lock
  diagnostic, uncaught runtime fault, or check failed).
- `2`: CLI usage error (invalid command-line arguments, missing file, unrecognized flag, or
  filesystem failure).

## Examples

Run a script:
```bash
aipo run main.aipo
```

Run a pre-compiled bytecode file:
```bash
aipo run main.aibc
```

Check a script with machine-readable diagnostics:
```bash
aipo check main.aipo --message-format=jsonl
```

Disassemble a source file (with line annotations):
```bash
aipo disasm main.aipo
```

Disassemble a bytecode file:
```bash
aipo disasm main.aibc
```

Format files in-place:
```bash
aipo fmt src/**/*.aipo
```

Build a JavaScript bundle and run it with Node (requires Node >= 20):
```bash
aipo build main.aipo --out dist
node dist/app.js
```

Create and audit a local package lockfile:
```bash
aipo package lock .
aipo package audit .
```

Lock a local root with pinned GitHub dependencies, then consume it offline:
```bash
cargo run -p aipo-cli --features github-http -- \\
  package lock . --fetch-github --cache .aipo-cache \\
  --github-token-env AIPO_GITHUB_TOKEN
aipo check src/main.aipo --package-cache .aipo-cache
aipo package cache verify .aipo-cache
aipo package cache prune .aipo-cache --lock aipo.lock
```

## Tools added in P07-G02

| Command | Contract |
|---|---|
| `aipo new <directory>` | Creates a local package with manifest, entry and README; does not overwrite an existing directory |
| `aipo plan <path>` | Shows the Reg/canonical plan as JSON without executing the program |
| `aipo profile <path> [--budget N] [--json]` | Runs on the VM and reports metrics; budget limits guest instructions |
| `aipo debug <path>` | stdio debugger with scheduler steps, break by byte offset and globals |
| `aipo watch <path>` | Watches the local tree, recompiles and reloads the last valid generation |
| `aipo lsp` | stdio LSP with full sync, UTF-16 diagnostics, completion and optional formatting |
| `aipo package vendor <package-dir> --out <new-directory> [--cache <dir>]` | Produces an offline tree from an existing lock, local dependencies and a SHA-256 inventory |

The tools use local package resolution and keep exit codes 0 (success), 1 (language failure) and 2
(tool usage/I/O). They do not automatically accept every `run` option; additional options are
rejected. LSP uses stdout exclusively for protocol frames.

`aipo-sh` preserves definitions/globals, accepts multiline input and offers `:help`, `:history`,
`:complete PREFIX`, `:load`, `:reload`, `:reset` and `:quit`. The persistent session uses
canonical services even in the Reg profile. History is JSONL with up to 1,000 units;
`AIPO_HISTORY_FILE` empty disables persistence.

See the [complete usage and reimplementation guide](../development/runtime-and-tooling-guide.md) for
Rust/C migration, rollback limits, profiles and distribution. Tests for this delivery were not
executed at the user's request; documented commands do not constitute certified results.
