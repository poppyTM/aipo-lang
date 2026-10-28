# P07-G03 — Release v0.1.0

**Data:** 2026-10-28 · **Estado:** DONE · **Tag:** `v0.1.0` · **Commit:** `819f4bc76c5a2587a3abba667289692b3da20f05`

## Publicação

- GitHub Release: https://github.com/poppyTM/aipo-lang/releases/tag/v0.1.0
- Release Pipeline: run `40631045953` — sucesso em todos os builds e no publish.
- Estável: `isDraft=false`, `isPrerelease=false`.

## Artefatos

| Plataforma | Arquivo |
| --- | --- |
| Linux x86_64 | `aipo-v0.1.0-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `aipo-v0.1.0-x86_64-pc-windows-msvc.zip` |
| macOS Intel | `aipo-v0.1.0-x86_64-apple-darwin.tar.gz` |
| macOS ARM64 | `aipo-v0.1.0-aarch64-apple-darwin.tar.gz` |
| Checksums | `SHA256SUMS.txt` e `*.sha256` |

## Gates registrados

- Workspace: 1.130 testes; Poppy, eGUI e game host incluídos.
- Conformance VM/Reg: 36/36 com saída igual; Reg nativo em 20/36.
- JavaScript: 36/36 programas e 23/23 exemplos.
- Wasm: 15/36 programas e 4/26 exemplos no subset documentado.
- MSRV 1.96: fmt, Clippy, testes, rustdoc estrito e perfis shell/web/embedded.
- Website: `npm run check` e `npm run build`.
- Prumo 0.6.0: `validate` e `doctor` limpos.
- CI do PR #45 e da `main` verde; Release Pipeline verde.

## Limites

Wasm/RegVM não têm paridade total; fuzz, Miri e ASan não foram executados; benchmarks são locais e ruidosos; consumidores C por plataforma ainda exigem validação.
