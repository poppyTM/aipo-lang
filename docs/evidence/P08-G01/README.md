# P08-G01 — Performance Matrix 2 baseline

**Data:** 2026-10-28 · **Estado:** DONE · **Base medida:** `c036164` (main após v0.1.0) · **Host:** AMD Ryzen 5 5600G, 12 CPUs, Linux x86_64 · **Pinagem:** `taskset -c 0` · **Rust:** 1.99.1 · **Build:** release.

> **Limite central:** este é o melhor host disponível com uma CPU isolada, não um runner dedicado. Os números são uma baseline reproduzível com escopo, não um gate de regressão.

## PERF-01 — Ambiente e protocolo

- `aipo-bench --json` executou 58 amostras em 7 rounds com CPU 0 pinada.
- `aipo-bench --compare --compare-quick --compare-runs 3` executou os workloads cross-language.
- Binário Aipo: `target/release/aipo`; hash SHA-256 registrado em `compare.json`.
- Load average no início: aproximadamente 1,16; a afinidade de CPU foi aplicada, mas o host permaneceu compartilhado.
- O próprio runner classificou o scaling como `noisy`; nenhum threshold foi aplicado.

Artefatos: [matrix2.json](matrix2.json) e [compare.json](compare.json).

## PERF-02 — VM vs RegVM fim-a-fim

Tempos medianos em milissegundos; processos incluem startup, parsing, compilação/plano e execução. `aipo-reg` inclui seleção de plano e fallback canônico quando aplicável.

| Workload | Aipo CLI/VM | Aipo VM in-process | Aipo RegVM | Reg/VM |
| --- | ---: | ---: | ---: | ---: |
| arithmetic | 102,68 | 99,46 | 97,62 | 0,95x |
| collections | 82,44 | 77,42 | 57,26 | 0,69x |
| fields | 254,42 | 254,45 | 259,41 | 1,02x |
| recursion | 36,96 | 33,76 | 57,19 | 1,55x |
| strings | 6,58 | 2,23 | 6,59 | 1,00x |

Leitura: Reg ganhou em `collections` e empatou em `strings`/`arithmetic`; perdeu em `recursion` e `fields`. O resultado não autoriza afirmar ganho universal. O plano Reg nativo continua em 20/36 programas de conformance.

## PERF-11 — Wasm

| Workload | Wasm JIT (ms) | VM CLI (ms) | Wasm/VM |
| --- | ---: | ---: | ---: |
| arithmetic | 16,74 | 102,68 | 0,16x |
| collections | 16,73 | 82,44 | 0,20x |
| fields | 16,68 | 254,42 | 0,07x |
| recursion | 16,75 | 36,96 | 0,45x |
| strings | **falhou** | 6,58 | — |

`strings` falha com `AIPO_COMPILE_WASM_UNSUPPORTED` por `len` fora do subset. Os tempos Wasm ficam próximos do piso de startup/JIT (~16,7 ms); não extrapole para workloads maiores.

## PERF-12 — Cross-language disponível

| Workload | Aipo VM | RegVM | Wasm | Aipo→JS | Python 3.12 | Node 18 | Rust nativo |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 102,68 | 97,62 | 16,74 | 358,05 | 31,87 | 108,99 | 6,24 |
| collections | 82,44 | 57,26 | 16,73 | 759,99 | 21,78 | 116,00 | 6,57 |
| fields | 254,42 | 259,41 | 16,68 | 619,01 | 62,23 | 110,20 | 6,60 |
| recursion | 36,96 | 57,19 | 16,75 | 236,00 | 16,71 | 110,06 | 6,40 |
| strings | 6,58 | 6,59 | — | 161,00 | 11,67 | 106,94 | 6,41 |

Runtimes indisponíveis neste host: Lua, LuaJIT, Luau, Wren, Ruby, PyPy e omniLua. Não foram instalados nesta rodada; a comparação existente com eles permanece histórica.

## Reprodução

```sh
cargo build --release -p aipo-cli -p aipo-bench
taskset -c 0 target/release/aipo-bench --json docs/evidence/P08-G01/matrix2.json
taskset -c 0 target/release/aipo-bench --compare --compare-quick \
  --aipo-bin target/release/aipo --compare-runs 3 \
  --compare-json docs/evidence/P08-G01/compare.json \
  --compare-languages aipo,aipo-vm,aipo-reg,aipo-wasm,aipo-js,python,javascript,rust
```

## O que este baseline não prova

- Não é runner dedicado, não mede RSS completo, alocação por runtime ou cold start multiplataforma.
- Não é gate de regressão e não substitui a matriz de performance completa da Matriz 2.
- Não certifica paridade Wasm/Reg; apenas mede o comportamento atual.
