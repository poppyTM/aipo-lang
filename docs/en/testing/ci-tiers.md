# CI tiers

**Status:** normative for CI design (process level)
**Scope:** which checks run where; no expensive test in every PR
**Related:** `docs/development/testing-strategy.md` (blocking gates),
`docs/testing/tooling-decisions.md`

> Performance runs are intentionally manual and never block shared PR runners. The remote
> workflow runs the deterministic quality tiers; the `perf` tier requires a dedicated machine.

## Tier `fast` (every PR)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked   # conformance, differential (Node >= 20), host, UI goldens, examples
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo build -p aipo-bench         # benchmark compilation only
prumo validate . && prumo doctor .
```

On Linux, install `libasound2-dev` before the Clippy/test steps so `aipo-game-host` links. Node
20+ is required for the JS differential suites. `cargo build -p aipo-bench` and the Prumo
commands are local gates; the current GitHub workflow runs the Rust gates above.

## Tier `nightly` (scheduled)

```bash
bash docs/testing/coverage.sh --report coverage.txt
cargo +nightly fuzz run lexer_tokens -- -max_total_time=600
cargo +nightly fuzz run frontend_check -- -max_total_time=600
cargo +nightly fuzz run formatter -- -max_total_time=600
cargo +nightly miri test -p aipo-vm
cargo audit
cargo deny check
cargo test --workspace --all-targets  # extended: generated-large seeds, resource suite
```

New crashes become minimized regression fixtures before the tier goes green again (regression
policy).

## Tier `perf` (dedicated runner only)

```bash
cargo build --release -p aipo-cli -p aipo-bench
target/release/aipo-bench --json target/aipo-bench.json
target/release/aipo-bench --compare --compare-json target/cross-language.json
```

The cross-language report follows `docs/performance/cross-language.md`: it includes Aipo CLI/VM,
Aipo VM in-process, Aipo→JavaScript/Node, Lua, LuaJIT, Python, Ruby, JavaScript/Node and native
Rust, with checksums and raw samples. Wall-clock gates never block shared runners. Blocking
performance gates require stable hardware, three historical baselines and an accepted threshold
per workload.
