# Runtimes de Referência — Tier B + C

**Rodada:** 2026-10-09. [Nove referências pinadas](../../studies/refs.json), leitura dirigida de source, sem builds/benchmarks upstream. Valores de tamanho ou velocidade anunciados por projetos não são medições desta revisão.
Luau e Janet (clonados e lidos fora do repo via `./studies/fetch-refs.sh`), notas docs-only de Tcl/PowerShell/Nushell (**sem clone** — foco em padrões de host), e análise dos runtimes WebAssembly. Pinagem em `studies/refs.json` e `refs.lock.json`.

> Desvios de layout registrados:
> - Luau: `Compiler/`, `Analysis/`, `VM/` contêm `include/` + `src/` (ex.: `VM/src/lbuffer.h`).
> - Janet: núcleo em `src/core/*.c` + API em `src/include/janet.h` (`vm.c`, `gc.c`, `fiber.c`, `bytecode.c` em `src/core/`).
> - WAMR: interpretador em `core/iwasm/interpreter/` (`wasm_interp_classic.c` / `wasm_interp_fast.c`); docs em `doc/`.
> - wasmi: crates em `crates/` (`wasmi`, `wasi`, `cli`, `core`, `ir`, …).

---

## 1. Luau (`luau-lang/luau`, `master`)

**O que é:** linguagem e runtime com compiler, análise estática e VM em componentes separados.

**Como funciona:**
- [`VM/src/lobject.h`](https://github.com/luau-lang/luau/blob/74f768309c380d40601b28c4c4cdda27da415cef/VM/src/lobject.h#L46) define `TValue` com payload, campos extras e tag.
- [`lbuffer.h`](https://github.com/luau-lang/luau/blob/74f768309c380d40601b28c4c4cdda27da415cef/VM/src/lbuffer.h#L6): limite `MAX_BUFFER_SIZE (1 << 30)`, layout `offsetof(Buffer, data) + max(len, 8)`, construtor/destrutor `luaB_newbuffer / luaB_freebuffer`, com implementação em `VM/src/lbuffer.cpp` e stdlib em `VM/src/lbuflib.cpp`. Buffer é uma categoria GC concreta de bytes com API de leitura/escrita, distinta de `String`.
- O `Analysis/` implementa inferência por restrições (`ConstraintGenerator`, `ConstraintSolver` em `Analysis/src/`), com fronteira pública em `Analysis/include/Luau/Frontend.h`: `struct Frontend`, `check(name)` retornando `CheckResult`, e o catálogo de erros em `Analysis/include/Luau/Error.h` (`TypeMismatch`, `TypeErrorData = Variant<…>`, `TypeErrorSummary`).
- `Compiler/src/` contém passes nomeados: `ConstantFolding`, `BuiltinFolding`, `ValueTracking`, `TableShape`, `CostModel`, com entrada `compile(...)` em `Compiler/include/Luau/Compiler.h`.

**Perguntas fechadas:**
- Buffer é categoria concreta de runtime, com limites explícitos; não é apenas texto com outro nome.
- Tipos graduais funcionam como contratos diagnosticados pelo frontend (`campo: Tipo` ⇒ erro diagnóstico, nunca coerção silenciosa em runtime).
- Otimizações são passes pequenos e testáveis, não uma flag monolítica.

**Lição → ação na Aipo:**
- Aipo preserva `Bytes` distinto de `String`; RegVM exige índice `Int` e valor `Byte` antes da escrita. Conferir limites e layout contra `lbuffer.h`. Crates: `aipo-vm`, `aipo-stdlib`.
- Contratos de tipo: `campo: Tipo` verificado no frontend com diagnósticos estilo `Error.h` (variante tipada + sumário), sem mudar semântica de runtime. Crates: `aipo-compiler` (diagnósticos), `aipo-vm` (`AssertContract`).
- Passes de dobra: `ConstantFolding`/`BuiltinFolding` como modelo de passes incrementais no emissor. Crates: `aipo-compiler`. Estudo aprofundado de `Analysis`/`Compiler`, diagnósticos e GC incremental continua como proposta.

---

## 2. Janet (`janet-lang/janet`, `master`)

**O que é:** runtime C enxuto com VM, fibers e GC, com representação de valores configurável.

**Como funciona:**
- [`janet.h`](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/include/janet.h#L311): 17 tipos (`JANET_NUMBER … JANET_POINTER`). Representação tripla — `JANET_NANBOX_64` (u64/i64/number/pointer), `JANET_NANBOX_32` (payload + tag u32) e fallback struct `{ as; type; }`. Verificação via `janet_checktype`/`janet_type`.
- Núcleo C enxuto: `src/core/` soma ~5.1k linhas em 4 arquivos (`vm.c` + `bytecode.c` + `gc.c` + `fiber.c`). Prova de que VM+GC+fibers cabem em arquitetura compacta.
- VM (`src/core/vm.c`): `VM_START` = `goto *op_lookup[first_opcode]`, `vm_next()` = `goto *op_lookup[*pc & 0xFF]`; fallback `switch(opcode)` em `for(;;)`. Confirmação de que o degrau `switch` → `goto` computado é mecânico e reversível.
- Fibers (`src/core/fiber.c`): `janet_fiber_reset` / `janet_fiber` / `janet_fiber_push`; pilha cresce via `setcapacity`/`janet_fiber_grow`. [`vm.c`](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/core/vm.c#L1321) expõe step/continue para fibers.
- GC (`src/core/gc.c`): marcação por tipo (`janet_mark_*`). [`janet_collect`](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/core/gc.c#L578) é a entrada explícita para investigar raízes e coleta.

**Perguntas fechadas:**
- Representação compacta depende do alvo e das premissas de ponteiro.
- Fibers exigem scheduler próprio; fiber C não equivale automaticamente ao scheduler determinístico de `Task` da Aipo.
- O degrau `switch` para `goto` computado é viável onde suportado pelo compilador C.

**Lição → ação na Aipo:**
- Manter NaN boxing como experimento separado, com gates e justificativa de portabilidade; o modelo de 3 modos (nanbox64/nanbox32/struct) é referência se a Aipo necessitar de build 32-bit (perfil nano).
- Preservar o Modelo B de falhas ao estudar suspensão. `fiber.c` serve de modelo conceitual mínimo para `Task` (reset/push/grow). Crates: `aipo-vm`.
- Orçamento de núcleo: ~5k linhas como teto de complexidade conceitual do `aipo-vm`.

---

## 3. Notas docs-only (sem clone)

Tcl, PowerShell e Nushell **não foram clonados** — estudados pela documentação oficial para calibração do host Aipo:

- **Tcl (reentrância do host)**: o interpretador Tcl é reentrante por desenho (`eval` aninhado, `Tcl_Eval` recursivo seguro). Ponto para o host AHS: chamadas host→Aipo→host devem ter profundidade limitada e estado de erro por invocação, não global.
- **PowerShell/Nushell (pipelines de valores estruturados)**: comandos trocam objetos estruturados/tipados, não texto cru. Ponto para a Aipo: stdlib de `task`/stream deve transportar `Value` tipado entre estágios, com contrato de forma (shape) verificável — convergindo com QuickJS shapes.

**Lição → Ação na Aipo:**
- Host reentrante com orçamento de profundidade + erro por invocação. Crates: `aipo-host`, `aipo-vm`.
- Pipelines tipados fim-a-fim. Crates: `aipo-stdlib`, `aipo-vm`.

---

## 4. Runtimes WebAssembly

### 4.1 Tabela comparativa

| Runtime | Tamanho / perfil | WASI | Configuração relevante | Papel na Aipo |
|---|---|---|---|---|
| `wasmtime` (referência externa) | ~25 MB (full-only) | sim | Runner JIT padrão | Teto full; runner opcional; nunca no nano/shell |
| `wasm3` (`main`) | ~64 KB código / ~10 KB RAM anunciados; pilha `d_m3MaxFunctionStackHeight 8000` | via `m3_api_wasi.c` | Flags em `source/m3_config.h` | Modelo de perfil determinístico; candidato nano se WASI bastar |
| `WAMR` (`main`) | Pequeno footprint, multiplataforma; RISC-V 32/64 | sim (libc-wasi, uvwasi) | `WAMR_BUILD_TARGET`, `WAMR_BUILD_FAST_INTERP`, `WAMR_BUILD_MINI_LOADER` | Candidato de experimento: WASI + configurável + RISC-V32 |
| `wasmi` (`main`) | Interpretador Rust puro, `no_std` no núcleo, fuel metering | sim (`crates/wasi`, `wasip1`) | Licença MIT/Apache-2.0; features no Cargo.toml | Alternativa Rust-nativa; isola `no_std` de adaptadores WASI |

*Nota metodológica:* Números de footprint ("~64 KB", "95 KB RISC-V32", "25 MB") vêm de documentação de terceiros e variam por flags/target. Devem ser medidos em benchmark com os mesmos binários Wasm antes de qualquer decisão.

### 4.2 wasm3 (`wasm3/wasm3`, `main`)

**O que é:** interpretador WebAssembly com configuração C e metacódigo próprio.

**Como funciona:** [`m3_config.h`](https://github.com/wasm3/wasm3/blob/28ecb9af6d2040e474a70f7cb7f43666740141fb/source/m3_config.h#L14) define limites de stack, páginas de memória, slots e validação. Pilha de função padrão `d_m3MaxFunctionStackHeight 8000`. Perfil determinístico documentado (`source/m3_config.h:162-185`, `source/m3_deterministic.h:1-8`) com relógio fixo e `random_get` com seed fixa.

**Pergunta fechada:** o custo de um runtime não se resume ao tamanho de seu executável; limites e flags de compilação fazem parte do perfil avaliado.

**Lição → ação:** qualquer experimento em Aipo precisa registrar build flags, memória, validação, imports e checksum. Perfil determinístico de wasm3 serve como referência para o modo replay da Aipo.

### 4.3 WAMR (`wasm-micro-runtime/wasm-micro-runtime`, `main`)

**O que é:** runtime WebAssembly com interpretadores clássico/rápido, AOT/JIT e integrações de host configuráveis.

**Como funciona:** [`runtime_lib.cmake`](https://github.com/wasm-micro-runtime/wasm-micro-runtime/blob/38b044a676893e678ceb19babaa431ca5e460114/build-scripts/runtime_lib.cmake) seleciona componentes; [`config_common.cmake`](https://github.com/wasm-micro-runtime/wasm-micro-runtime/blob/38b044a676893e678ceb19babaa431ca5e460114/build-scripts/config_common.cmake#L301) condiciona fast interpreter ao interpretador habilitado. Libc WASI e uvwasi são configurações separadas.

| Dimensão | Gate observado | Implicação para um experimento |
| --- | --- | --- |
| Interpretador | `WAMR_BUILD_INTERP` | Definir modo de execução |
| Fast interpreter | `WAMR_BUILD_FAST_INTERP` | Comparar a mesma carga com flags registradas |
| AOT/JIT | `WAMR_BUILD_AOT`, `WAMR_BUILD_JIT` | Separar startup, compilação e execução |
| WASI | `WAMR_BUILD_LIBC_WASI`, `WAMR_BUILD_LIBC_UVWASI` | Definir imports e permissões do host |
| Alvo | `WAMR_BUILD_TARGET` | Não transportar tamanho de RISC-V para x86-64 |

**Pergunta fechada:** há configurações com e sem WASI. WAMR é um candidato de experimento, não uma substituição automática. A licença contém partes com `Apache-2.0 WITH LLVM-exception`.

**Lição → ação:** produzir um spike isolado antes de propor troca de engine; registrar imports, footprint e licença por arquivo. Nenhum código C upstream é copiado para o repositório Aipo.

### 4.4 wasmi (`wasmi-labs/wasmi`, `main`)

**O que é:** interpretador WebAssembly em Rust puro, com núcleo e adaptador WASI separados no workspace.

**Como funciona:**
- [`crates/wasmi/src/lib.rs`](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasmi/src/lib.rs#L87) declara `#![no_std]`, usa `alloc` e habilita `std` por feature.
- O [manifest do núcleo](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasmi/Cargo.toml#L37) inclui gates como `std`, `wat`, `validate`, `memory64`, `auto-dispatch`, `portable-dispatch`, `indirect-dispatch` e `deterministic`.
- O [adaptador WASI](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasi/Cargo.toml) depende do núcleo com `std`, de `wasi-common` e de `wiggle`.
- A [licença do workspace](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/Cargo.toml#L25) é dual `MIT/Apache-2.0`.

**Perguntas fechadas:** a expressão “wasmi com WASI” não anula o suporte `no_std` do núcleo; integração WASI e núcleo são componentes distintos.

**Lição → ação:** incluir wasmi no comparativo com WAMR antes de escolher runtime para shell/nano. O runner Wasmtime existente recebeu suporte a fuel/budget e isolamento de features; engines alternativas permanecem em fase de spike investigativo.

---

## 5. Protocolo de comparação futuro

Fixar o mesmo binário `.wasm`, entradas, checksum e imports; medir cold/warm start separadamente; registrar flags de compilação, dependências, arquitetura-alvo, toolchain e consumo de memória; verificar licenças e tratamento de erros por limites. Não desabilitar validação em um candidato e mantê-la em outro sem registrar explicitamente a divergência.
