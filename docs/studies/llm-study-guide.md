# Aipo — Guia Master para LLMs: Estudo Profundo e Implementação

**Versão:** 2.2 · **Data:** 2026-10-09 · **Status:** normativo-operacional
**Público:** qualquer LLM que for executar estudo aprofundado e implementação neste repositório.
**Pré-requisito:** ler este documento INTEIRO, nesta ordem, antes de qualquer ação.
**Base estudada:** commit `488905ffb0b83ebbff458939542350d7280ad79b`.
**Base de integração:** `bcbc4c991dee1d9f853c597aaed2790e8981b8b9`, após incorporar a atualização de dependência já publicada na `main`.

**Rodada atual P07-G02:** [guia completo de uso/reimplementação](../development/runtime-and-tooling-guide.md), [continuidade dos estudos](runtime-and-tooling-follow-up.md) e [evidência reproduzível](../evidence/P07-G02/README.md). Base: `f0a0d70d176be031cd4b600fde1df6179429fb9a`, após merge da P07-G01. Testes explicitamente não executados; goal permanece DRAFT sem aprovações/gates simulados. A documentação anterior é histórica.

---

## 0. Como usar este documento

1. Leia as seções 1–4 para contexto e regras (obrigatório, sem exceção).
2. Execute a Seção 5 (fetch do corpus) e confira o lock.
3. Estude na ordem da Seção 6 (Tier A → B → C), preenchendo o template da Seção 7.
4. Implemente segundo a Seção 8 (propostas por dimensão) e as receitas da Seção 10.
5. **Definição de "pronto":** Seção 12 (contrato de saída) 100% atendida.

> Convenção de paths: `$AIPO_ROOT` = raiz deste repositório;
> `$AIPO_REFS` = clones de referência (padrão `~/aipo-refs`, via `AIPO_REFS_DIR`).
> Nunca hardcode `/home/...`.

---

## 1. Missão, não-metas e filosofia

**Missão:** estudar VMs/runtimes de referência em profundidade e implementar
as peças faltantes (RegVm, `aipo-wasm`, perfis de embedding, interop C/Rust),
**sem fragmentar a linguagem nem o repositório**.

**Não-metas:** redesenhar sintaxe (congelada pelo ADR-001); criar crates por
perfil; trocar o Modelo B de falhas; adicionar dependências pesadas sem ADP.

**Filosofia (não-negociável, fonte: ADR-001, ADP-012, `docs/llm/`):**
- **ND-first:** cada decisão de superfície passa por revisão de sobrecarga
  cognitiva (TDAH, dislexia). Uma forma só para cada coisa; palavra completa
  vence símbolo; zero cerimônia para o caso comum; o erro diz o que fazer.
- **Determinismo:** mesma entrada, mesma saída (VM single-thread por decisão;
  `Value` é `!Send` — um runtime por thread).
- **No-invention:** questão semântica sem resposta no canon vira ADP/issue,
  nunca código silencioso.
- **Evidence-first:** claims de performance/segurança exigem medição anexada
  (`scripts/perf/paired.sh` + `cpu_ab.py`: ≥2 lotes, ±5% = ruído).

---

## 2. Snapshot do projeto

Pipeline: `lexer → syntax → hir → sema → ir → bytecode → {stack-VM, RegVm, JS, Wasm}`.
Toolchain: MSRV Rust 1.96, alinhado ao lock Wasmtime 49.0.2/Cranelift 0.136.2. Compilação local com Rust 1.96 e revisão Clippy com 1.99; Node >= 20 para suítes JS. Testes não executados nesta rodada.
24 crates no workspace (`unsafe_code = "forbid"`, clippy `deny` em tudo).

| Crate | Papel |
|---|---|
| `aipo-lexer/syntax/ast/hir/sema` | Frontend compartilhado (semântica única) |
| `aipo-ir` | Core IR neutro (43 instruções) |
| `aipo-bytecode` | Stack emitter + `RegEmitter` + `RegOpCode` u32 |
| `aipo-vm` | Stack VM (referência) + `RegVm` + `Value` (layout por alvo) + `VmMetrics` + budget |
| `aipo-js` | `emit_js` → `JsBundle` (paridade diferencial com a VM) |
| `aipo-wasm` | `compile_hir`, `WasmEmitter`, `execute_wasm`/`execute_wasm_with_options` (wasmtime feature-gated) |
| `aipo-cli` | Bins `aipo` (full) e `aipo-sh` (shell, REPL); subcomando `test` |
| `aipo-host` | `Capability`/`CapabilitySet` (deny-by-default), `HostValue` (7 variantes), `Handle` |
| `aipo-c-abi` | C ABI síncrona/cooperativa + Reg raw; exports devem coincidir com `include/aipo.h` (ver §8.5) |
| `aipo-diagnostics` | Catálogo normativo + JSONL + locale En/PtBr |
| demais | `runtime, stdlib (módulo sh: run/cd/pwd/env/which), formatter, package, poppy, testkit, bench, game-host, egui` |

**Avisos práticos:** `target/` passa de 40 GB (se `ENOSPC`, apague
`target/debug/incremental`); builds com wasmtime levam minutos (timeout 300 s+);
prefira `cargo test -p <crate> --test <suite>`.

---

## 3. Regras de ferro

### 3.1 Anti-alucinação
1. Todo fato cita `path:linha` lido via Read/Grep. 2. `ls` antes de assumir
qualquer path. 3. Nunca invente APIs/flags. 4. Números só com método junto
ou marcados "estimativa".

### 3.2 Tabus da sintaxe (ADR-001)
| PROIBIDO | Use |
|---|---|
| `end` | `}` |
| `for x in xs`, `repeat N:`, `while` com `do` | `each x in xs { }`, `repeat N as i { }`, `while c { }` |
| `// comentário`, `/* */` | `#` (`//` é divisão inteira!) |
| `try/catch/throw`, `defer` | `attempt { } failed e { }`, `fail`, `?`, `or_else` |
| `class`, `impl T { }`, `satisfy`, `self!`, `div`, `&& \|\| !`, `;`, `?:` | `struct`, `T:m()`, `#!satisfies`, `var self`, `//`, `and or not`, quebra de linha, `if c then a else b` |
| `else if`, `fn T:m()` | `elif`, `T:m()` |

### 3.3 Regras do repositório
- NÃO commite/pushe sem pedido explícito. Docs PT-BR, código EN.
- Gramática/semântica/IR/bytecode/diagnóstico/CLI mudam só com fixture de
  conformance + testes (`docs/conformance/`).

---

## 4. Estado da implementação

### 4.1 Pronto (verificar, não refazer)
Sintaxe T1–T7; `enum` V1 (VM+JS+Wasm); `?`, `with`, destructuring+guards;
diagnósticos §4bis + narrowing; batch `Tipo::[...]`; RegVm (aritmética, saltos,
chamadas, defaults, `each`, `TypeIs`, coleções/structs, falhas, handlers);
Wasm (enum por tag-pointer, `[]/{}` vazios, `.len()`); `aipo-sh --engine=reg`.

### 4.2 Implementação atual e limites

- RegEmitter estrito + RegVerifier: operandos/pools/jumps/functions, inicialização definida pelo CFG e escopos de handlers/iterações. Entradas externas são verificadas antes de efeitos.
- `ExecutionPlan` representa Reg nativo ou bytecode canônico com razão. O perfil Reg executa a superfície compartilhada; captures, async, interfaces, hooks e journal não ganharam paridade no interpretador Reg nativo.
- Chamadas Reg movem/restauram janelas vivas; corpos usam Rc e pools guest preparados por módulo. Microbenchmark Int/String contra base pinada, três lotes, está na evidência P07-G02; não extrapolar para toda a linguagem.
- Coleções usam revisão estrutural, independente de comprimento. Snapshot guest preserva aliases/ciclos por restauração em lugar; não é GC e não reverte recursos externos do host.
- Session mantém globals/catálogo/linker, multilinha/histórico e recarga com migração/rollback. `new`, `watch`, `plan`, `profile`, `debug`, LSP básico e vendor offline estão implementados.
- Reg/C têm budgets e erros explícitos; C oferece begin/pump/abort. Wasm oferece fuel/memory/output e Engine/cache reutilizáveis. Callback nativo não é limitado pelo budget de opcodes.
- `is` Wasm compara tipo semântico conhecido; tipos desconhecidos/joins ambíguos são rejeitados.
- `Value`/frames são medidos por alvo, não há promessa multiplataforma de 16 bytes.
- `no_std`, GC de ciclos, allocator guest plenamente limitado e porte MCU permanecem evolução futura. `profile-embedded` e `nano` usam `std`.
- Testes novos e conformance foram escritos/compilados, não executados. A matriz OS está definida, não certificada por resultados locais.

### 4.3 Estudos disponíveis

[reference-vms.md](reference-vms.md), [reference-runtimes.md](reference-runtimes.md) e [lessons-for-aipo.md](lessons-for-aipo.md) registram leitura dirigida e ações. Não certificam auditoria integral upstream nem compilação de runtimes externos.

**Correções de premissas:** WAMR não é vencedor medido nem único candidato com WASI. wasmi tem núcleo `no_std` e adaptador WASI separado, com `std`. ADR-001 e ADP-012 citados na versão anterior não estão materializados neste checkout; aplicar o canon/authority map existente, sem inventar arquivos ou decisões.

---

## 5. Corpus de referência

Fetch: `./studies/fetch-refs.sh` → `$AIPO_REFS` (**fora do repo, por decisão**).
Lock: `$AIPO_REFS/refs.lock.json`. Re-pinar só conscientemente, com data.

| Ref | Repo | Branch | SHA 2026-10-08 | Licença | Tier | Foco |
|---|---|---|---|---|---|---|
| `lua` | `lua/lua` | `master` | `0b29f408433e` | MIT (`COPYRIGHT`) | A | `TValue`, upvalues, `lvm.c` |
| `luajit` | `LuaJIT/LuaJIT` | `v2.1` | `c6ffc141a876` | MIT/X custom | A | janelas de reg, dispatch, `lj_func.c` |
| `wren` | `wren-lang/wren` | `main` | `99d2f0b8fc26` | MIT | A | VM mínima, single-pass, fibers |
| `quickjs` | `quickjs-ng/quickjs` | `master` | `dad13e33cee3` | MIT | A | refcount+ciclos, shapes, átomos |
| `luau` | `luau-lang/luau` | `master` | `74f768309c38` | MIT | B | gradual typing, diagnósticos, `buffer` |
| `janet` | `janet-lang/janet` | `master` | `246951c8090a` | MIT | B | tagged union, fibers, núcleo C |
| `wasm3` | `wasm3/wasm3` | `main` | `28ecb9af6d20` | MIT | C | limites e flags; tamanho a medir |
| `wamr` | `wasm-micro-runtime/wasm-micro-runtime` | `main` | `38b044a67689` | **Apache-2.0** | C | flags interp/AOT/JIT e WASI opcional |
| `wasmi` | `wasmi-labs/wasmi` | `main` | `2970aa871cc1` | MIT/Apache-2.0 | C | núcleo Rust no_std; WASI separado |

Layouts que desviam do óbvio: `lua/` é flat na raiz; QuickJS-ng sem
`internals.md` (ler no fonte); Luau aninhado (`VM/src/`…); Janet em
`src/core/`; WAMR em `core/iwasm/interpreter/`.

---

## 6. Metodologia

Ordem: Wren → QuickJS → Lua → LuaJIT → Luau → Janet → runtimes.
Template por linguagem: *O que é / Como funciona (código real) /
Perguntas fechadas / Lição → Ação (com crates)*. Sem "Lição → Ação",
é leitura passiva e não entra.

---

## 7. Briefs (arquivos exatos + perguntas)

- **Lua** (`lobject.h, lstate.h, lvm.c, lfunc.c, lgc.c`): layout `TValue` em
  64-bit? lista ordenada de upvalues? pior-caso do `findupval`? `switch` vs
  `goto` no `luaV_execute`? `MAXUPVAL=255` cabe no u32?
- **LuaJIT** (`src/lj_obj.h, lj_func.c, lj_bc.h, lj_gc.c, lj_vm.h`): base-de-frame
  + slots vs nossa cópia? formato da palavra BC? `GCRef` 32-bit vs `Rc`?
- **Wren** (`src/vm/wren_vm.c` por inteiro + `wren_compiler.c, wren_value.h`,
  `src/include/wren.h`): largura do `Value`? frames? fibers vs `Task`?
  patch de jumps? inline caches?
- **QuickJS-ng** (`quickjs.c, quickjs-opcode.h, libregexp.c`): shapes ≈
  `struct_fixed_fields`? átomos ≈ internar campos? gatilho do cycle-removal?
- **Luau** (`Compiler/, Analysis/, VM/`, ≤8 arquivos): entry do typechecker,
  `buffer` ≈ `Bytes`, formato de diagnósticos.
- **Janet** (`src/core/vm.c, gc.c, fiber.c`, `src/include/janet.h`): union
  (3 modos?), mark por tipo, fibers.
- **wasm3** (`source/m3_config.h`, dispatch): tamanhos, profile determinístico.
- **WAMR** (`core/iwasm/`, opções de build): custo WASI, menor config.
- **wasmi** (`crates/`): separar núcleo `no_std` do adaptador WASI/std e medir por configuração.
- **Tcl/PS/Nushell:** docs-only (reentrância; pipelines de valores; `pipe2`).

---

## 8. Propostas de implementação por dimensão

Formato de cada item: *O quê / Por quê (fonte) / Onde / Pronto-quando.*

### 8.1 Performance
- **P1 Janelas de registrador (Lua/LuaJIT):** implementada janela viva com move/restore e constantes compartilhadas em `reg_vm.rs`; evidência P07-G02 com três lotes. Não é ainda uma stack única de frames sem cópia; conformance executada continua pendente.
- **P2 Dispatch:** threaded/`goto` computado (Wren c/ fallback, WAMR
  classic-vs-fast) + estreitar `Result` do hot path (cf. ADP-011, piso ~100ns).
- **P3 Acesso a campo:** acabar com busca linear (`value.rs:93,109`) via
  shapes/slots/átomos (QuickJS); verificar/estender cache de slots.
- **P4 Strings:** internar nomes de campo/tipo; auditar custo NFC (feature `unicode`).
- **P5 Métodos:** estender `StructMethod` sem-alloc + inline cache monomórfico.
- **P6 Bytecode:** dedup de constantes + fusão (`GetField+Call`); `Call0..4`.
- **P7 NaN-boxing opcional** (precedente: Janet tem 2 modos) como experimento tier-gated.

### 8.2 Acessibilidade ND
- **A1** Auditar catálogo: todo erro com `suggestion` preenchida.
- **A2** Todo código novo exige strings En+PtBr (checklist §10).
- **A3** Gate ND em ADPs de sintaxe (racional TDAH/dislexia obrigatório).
- **A4** Formatter canônico + idempotência em CI (redutor de sobrecarga de escolha).
- **A5** REPL amigável (multilinha, histórico, erros com sugestão) — verificar/estender.
- **A6** `aipo test` legível + passe de leitura no manual.
- **A7** Lista de vocabulário canônico (um termo por conceito).

### 8.3 Velocidade/startup
- **V1** Baseline de cold start por perfil (meta: shell <2 ms).
- **V2** Aliases `profile-shell`, `profile-web`, `profile-embedded` com `--no-default-features` implementados; `nano` é perfil Cargo. Default continua incluindo Wasmtime.
- **V3** Registro lazy de stdlib/natives (medir custo eager atual).
- **V4** `.aibc` pré-compilado + cache (verificar/estender serialização).
- **V5** AOT **não** é estratégia de startup (custo de compilação); startup rápido = interpretador (spike WAMR).

### 8.4 Segurança
- **S1** Supply chain: manter deny/audit/review; auditar grafo de deps do perfil nano; política de lockfile.
- **S2** Fuzz: aos 3 alvos atuais, somar *bytecode verifier*, *VM step* com corpus,
  *diferencial VM-vs-RegVm*, *validação AHS*.
- **S3** Capabilities: estender a novas superfícies; tier por perfil; fuzz no AHS.
- **S4** Robustez de input: ADP-005 + limites de tamanho; guards de profundidade na RegVm.
- **S5** Checklist por export C (null, UTF-8, re-entrância, unwind) — formalizar o padrão de `c_api.rs`.
- **S6** Orçamentos em tudo: levar budget da Stack VM/C ABI à RegVm e ao runner wasm (fuel).
- **S7** Manter `unsafe_code = "forbid"` + Miri p/ RegVm no CI.
- **S8** Varredura de segredos (cf. threat-model) — verificar hook e estender.

### 8.5 Interop C (header como fonte da superfície)
Grupos: lifecycle, carga/chamada, capacidades, handles/values, budgets/erros,
Reg raw e begin/pump/abort cooperativos. Compare os símbolos de `c_api.rs`
com `include/aipo.h`; não manter contagem textual desatualizada.
- **C1 Entregue/limites:** begin/pump/abort e budget/erro Reg; header compilado como C/C++ na evidência. Streaming, versionamento de callbacks e provas nativas executadas continuam pendentes.
- **C2 Padrões verificados:** Wren slots+handles (`wrenEnsureSlots`,
  `WrenHandle`); QuickJS runtime/context + `Dup/Free` (ownership explícita);
  WAMR `exec_env`+`custom_data`+natives por módulo; Lua `lua_State` por thread.
- **C3 Propostas:** API variádica por slots; teste de compat header-vs-impl;
  política de threads documentada (`Value: !Send` → 1 runtime/thread);
  política de re-entrância; regras de ownership de erros; versionamento de callbacks.
- **C4** Natives com namespace por módulo (padrão WAMR).

### 8.6 Interop Rust
- **R1** Fachada mínima (`aipo::eval`), política semver/MSRV documentadas.
- **R2** Superfície a budget + `VmMetrics` na API pública.
- **R3** `std::error::Error` + relatórios opcionais; manter JSONL.
- **R4** Roadmap `no_std` (split core/alloc), auditoria de deps por feature.

### 8.7 Outras superfícies
- **JS:** callbacks bidirecionais (verificar), versionamento do shim, source maps.
- **Formatter/test/REPL/i18n:** completude, idempotência, espelho EN sincronizado
  (checklist p/ toda string visível), determinismo documentado.
- **Packages/observabilidade:** auditoria de registry; métricas por perfil; logs estruturados.

---

## 9. Síntese + P2/P3/P4

- Gap→estudo: cada item da §8 referencia a lição que o informa.
- **P2:** nano = build nativo com size optimization; tamanho/startup exigem medição real. MCU/`no_std` não estão implementados.
- **P3:** wasmtime feature-gated; comparar spikes WAMR e wasmi antes de escolher engine.
- **P4:** 1 semântica/N emissores; tiers 1/2/3 com erro amigável; conformance como
  árbitro; perfis via features+`[profile.*]`, nunca crates novas; CI `full/shell/nano`.

---

## 10. Receitas de implementação

**Nova instrução na RegVm:** `RegOpCode` (+`from_u8`) → lowering no `RegEmitter`
(A/B/C + `top`, sem quebrar `core_to_reg`) → operandos/CFG no `RegVerifier` → handler em `RegVm::run_verified` → unit em
`reg_vm.rs` → e2e em `shell_tests.rs` (`eval_source_reg`) → conformance → fmt+clippy.
**Nova construção no Wasm:** layout (Pass 0) → string estática → `compile_expr` →
slots em `pre_scan_stmts` → teste Wasmtime real.
**Novo export C:** checklist S5 + teste de pin da ABI + entrada em `aipo.h`.
**Novo diagnóstico:** código + strings En+PtBr + fixture + entrada no catálogo.
**Novo feature-gate:** feature + matriz CI + erro amigável quando ausente (P4.2).

---

## 11. Fases e aceite

| Fase | DoD |
|---|---|
| 1 Infra | fetch limpo; lock 9 SHAs; diff só `studies/`+`docs/studies/`+sidebar |
| 2 Tier A | 4 seções, fatos com `path:linha`, Lição→Ação cada |
| 3 Tier B/C | idem + tabela wasm com flags verificadas |
| 4 Síntese | `lessons-for-aipo.md` + ADPs/issues |
| 5 Implementação | §8 por dimensão, cada item com teste + conformance verdes |

---

## 12. Contrato de saída

1. Arquivos criados/alterados + diff stat. 2. Saídas de verificação.
3. Desvios vs este guia + fatos que contradizem a §4. 4. Pendências explícitas.
5. Nunca commitar/pushear sem pedido.

## Apêndice — fontes e limites de verificação

Use símbolos, não números de linha antigos: `RegEmitter::stack_heights/compile_function`, `RegVm::run/invoke_function/set_instruction_budget`, `value::string_char_at`, `execute_wasm_with_options` e `studies/refs.json`. Os links dos estudos incluem SHA upstream e regiões efetivamente lidas.

A seção 11 conserva os gates de certificação futura. A publicação atual é draft, com dispensa explícita da execução de testes; não equivale à aprovação de todas as fases deste guia.

## Continuidade P07-G02

O [guia de reimplementação](../development/runtime-and-tooling-guide.md) detalha as cinco frentes, a migração dos payloads Rust para `Collection`, a seleção explícita do engine, o linker/snapshot e os limites de distribuição. As receitas e metas futuras desta página não devem ser anunciadas como recursos entregues. Aprovação para commit/push/PR foi dada pelo usuário; execução de testes foi explicitamente dispensada. Nenhum resultado histórico ou perdido certifica os arquivos atuais.
