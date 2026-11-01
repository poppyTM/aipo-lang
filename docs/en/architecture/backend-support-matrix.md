# Backend support matrix

**Status:** operational evidence, not a universal certification.
**Measurement date:** 2026-10-31.
**Source:** `aipo plan`, `aipo check --wasm`, `aipo run --engine=vm|reg`, `cargo test --workspace` and the `docs/conformance/` corpus.

## How to read

- **Native** means the chosen backend ran the program without fallback.
- **Fallback** means the plan selected the canonical VM with an explicit reason.
- **Equal output** means byte-identical stdout against the committed `.stdout`.
- The matrix measures this repository's corpus and examples; it does not promise coverage of the whole language.

## Conformance programs

| Backend | Native coverage | Fallback | Equal output | Notes |
| --- | ---: | ---: | ---: | --- |
| Stack VM | 36/36 | 0 | 36/36 | Reference semantics |
| RegVM | 20/36 | 16/36 | 36/36 | Fallback: higher-order methods, failure handlers and non-representable IR |
| JavaScript | 36/36 | 0 | 36/36 | ESM bundle; requires Node with ESM |
| WebAssembly | 29/36 | — | 29/36 | Subset; compile failures emit `AIPO_COMPILE_WASM_UNSUPPORTED`. Covers `init`/`invariant` with mutation rollback, the full `string` module (including NFC normalization, case folding and Unicode trim), positional `Dict`, `await`/`task.spawn`/`task.all` and `T?` contracts. Declared (not silent) limits: nested `fn` or a closure that captures a variable from the outer frame, and async combinators that require a scheduler. Each is rejected with `AIPO_COMPILE_WASM_UNSUPPORTED` and the recipe `use --engine=vm`, instead of producing output different from the VM |

## Executable examples

| Backend | Coverage | Equal output | Notes |
| --- | ---: | ---: | --- |
| Stack VM | 23/23 with `.stdout` | 23/23 | 3 host examples have no snapshot |
| RegVM | 10/23 native; 13/23 fallback | 23/23 | Explicit fallback preserves output |
| JavaScript | 23/23 | 23/23 | The build writes `package.json` with `"type": "module"` |
| WebAssembly | 25/26 | — | Host programs and closures with capture still do not compile; NFC is now supported |

## Other subsystems

| Area | Evidence | Limit |
| --- | --- | --- |
| C ABI | Rust tests + native C harness + `header_sync` (36 exports) | Validate the external consumer on each platform |
| Packages | Resolver, lock, cache, vendor and pinned-GitHub tests | No public registry |
| Host ABI/Poppy | 14 green Poppy tests | Capabilities remain deny-by-default |
| eGUI | 9 green tests | Headless adapter, no OS window |
| Game host | 43 bridge tests + 3 audio | Run on 2026-10-27 with `libasound2-dev` installed on Linux |

## Reproduction

```sh
cargo test --locked
cargo test -p aipo-poppy
cargo test -p aipo-egui
target/debug/aipo plan docs/conformance/programs/01_hello.aipo
target/debug/aipo check examples/01_fizzbuzz.aipo --wasm
target/debug/aipo run docs/conformance/programs/01_hello.aipo --engine=reg
```

The Reg plan only selects the native backend when the emitter and verifier accept the IR and the
construction does not depend on canonical services that are absent. When there is a fallback, `aipo plan`
shows the reason. The existence of the flag is not proof of parity.
