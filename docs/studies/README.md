# Estudos de Linguagens de Referência

**Status:** vivo (metodologia) · **Autoridade:** Nível 6 (registro) — nada aqui redefine semântica; conclusões viram ADP/issue antes de virar código.
**Pré-requisito:** `./studies/fetch-refs.sh` (clones fora do repo, por padrão em `~/aipo-refs`).

Esta rodada relaciona inspeção de fontes pinadas às correções e às próximas propostas. Leituras de código não são resultados de benchmarks.

## Por que existe

Calibrar a `RegVm`, o `aipo-wasm` e os perfis de embedding contra implementações reais, em vez de reinventar. Regra: **nenhuma decisão entra no código sem ADP/issue; este diretório registra o estudo, não a decisão.**

## Guias e Arquivos deste diretório

- [Guia master para LLMs](llm-study-guide.md): metodologia, limites, corpus e fases.
- [Guia de uso e reimplementação](runtime-hardening-guide.md): mudanças concretas desta revisão.
- [VMs de referência](reference-vms.md): Wren, QuickJS-ng, Lua e LuaJIT (Estudos Tier A).
- [Runtimes de referência](reference-runtimes.md): Luau, Janet, wasm3, WAMR e wasmi (Tier B + C).
- [Lições para Aipo](lessons-for-aipo.md): síntese acionável, ações implementadas e propostas pendentes.
- [Manifest de fontes](../../studies/refs.json): SHAs completos das nove referências pinadas.

## Manifest de Referências

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
| `wasmi` | `wasmi-labs/wasmi` | `main` | **MIT/Apache-2.0** | C | Rust puro; `no_std` no núcleo, `crates/wasi` separada |

SHA pinado de cada clone: `studies/refs.json` (no repo) e `refs.lock.json` no diretório de destino fora da árvore (gerado pelo script).

Use `./studies/fetch-refs.sh` na raiz para obter o corpus fora do repositório e `--verify` para conferir um checkout já materializado sem rede. Não copie upstreams para o workspace Aipo nem transforme tamanhos anunciados por terceiros em medidas da nossa implementação.

## Disciplina de cada seção

Toda linguagem termina em **“Lição → Ação na Aipo”** com crates afetadas. Sem isso, é leitura passiva e não entra aqui.

## Continuidade P07-G02

Veja [runtime-and-tooling-follow-up.md](runtime-and-tooling-follow-up.md), o [guia de uso/reimplementação](../development/runtime-and-tooling-guide.md) e [evidência](../evidence/P07-G02/README.md).
