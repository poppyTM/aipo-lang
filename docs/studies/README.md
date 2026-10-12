# Estudos de Linguagens de Referência

**Status:** vivo (metodologia) · **Autoridade:** Nível 6 (registro) —
nada aqui redefine semântica; conclusões viram ADP/issue antes de virar código.
**Pré-requisito:** `./studies/fetch-refs.sh` (clones em `~/aipo-refs`, fora do repo).

## Por que existe

Calibrar a `RegVm`, o `aipo-wasm` e os perfis de embedding contra
implementações reais, em vez de reinventar. Regra: **nenhuma decisão
entra no código sem ADP/issue; este diretório registra o estudo, não a decisão.**

## Manifest (verificado em 2026-10-08 via API do GitHub)

| Ref | Repo | Branch | Licença | Tier | Foco do estudo |
|---|---|---|---|---|---|
| `lua` | `lua/lua` | `master` | MIT (`COPYRIGHT`) | A | `TValue`, upvalues abertas, `lvm.c` |
| `luajit` | `LuaJIT/LuaJIT` | `v2.1` | MIT/X custom | A | register windows, threaded dispatch, `lj_func.c` |
| `wren` | `wren-lang/wren` | `main` | MIT | A | VM < 4k `;`, single-pass, fibers |
| `quickjs` | `quickjs-ng/quickjs` | `master` | MIT | A | refcount + cycle-removal, shapes, atoms |
| `luau` | `luau-lang/luau` | `master` | MIT | B | gradual typing, diagnósticos, `buffer` |
| `janet` | `janet-lang/janet` | `master` | MIT | B | tagged union, fibers, núcleo C |
| `wasm3` | `wasm3/wasm3` | `main` | MIT | C | ~64 KB / 10 KB RAM, deterministic profile |
| `wamr` | `wasm-micro-runtime/wasm-micro-runtime` | `main` | **Apache-2.0** | C | 95 KB em RISC-V32, **com WASI** |
| `wasmi` | `wasmi-labs/wasmi` | `main` | **Apache-2.0** | C | Rust puro; **manutenção apenas, sem WASI** |

SHA pinado de cada clone: `~/aipo-refs/refs.lock.json` (gerado pelo script).

## Arquivos deste diretório

| Arquivo | Conteúdo |
|---|---|
| `reference-vms.md` | Estudos Tier A (Lua, LuaJIT, Wren, QuickJS) |
| `reference-runtimes.md` | Tier B + C (Luau, Janet, Tcl/docs, PowerShell/docs, wasm3, WAMR, wasmi) |
| `lessons-for-aipo.md` | Síntese acionável + matriz de perfis + regras anti-fragmentação |

## Disciplina de cada seção

Toda linguagem termina em **“Lição → Ação na Aipo”** com crates
afetadas. Sem isso, é leitura passiva e não entra aqui.
