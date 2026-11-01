# P07-G03 — Release v0.1.0

**Date:** 2026-10-28 · **State:** DONE · **Tag:** `v0.1.0` · **Commit:** `819f4bc76c5a2587a3abba667289692b3da20f05`

## Publication

- GitHub Release: https://github.com/poppyTM/aipo-lang/releases/tag/v0.1.0
- Release Pipeline: run `40631045953` — success on every build and on publish.
- Stable: `isDraft=false`, `isPrerelease=false`.

## Artifacts

| Platform | File |
| --- | --- |
| Linux x86_64 | `aipo-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `aipo-v0.1.0-x86_64-pc-windows-msvc.zip` |
| macOS Intel | `aipo-v0.1.0-x86_64-apple-darwin.tar.gz` |
| macOS ARM64 | `aipo-v0.1.0-aarch64-apple-darwin.tar.gz` |
| Checksums | `SHA256SUMS.txt` and `*.sha256` |

## Registered gates

- Workspace: 1,130 tests; Poppy, eGUI and game host included.
- Conformance VM/Reg: 36/36 with equal output; native Reg on 20/36.
- JavaScript: 36/36 programs and 23/23 examples.
- Wasm: 15/36 programs and 4/26 examples in the documented subset.
- MSRV 1.96: fmt, Clippy, tests, strict rustdoc and shell/web/embedded profiles.
- Website: `npm run check` and `npm run build`.
- Prumo 0.6.0: `validate` and `doctor` clean.
- CI of PR #45 and of `main` green; Release Pipeline green.

## Limits

Wasm/RegVM do not have full parity; fuzz, Miri and ASan were not run; benchmarks are local and
noisy; C consumers per platform still require validation.
