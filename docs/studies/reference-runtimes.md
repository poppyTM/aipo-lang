# Runtimes de Referência — Tier B + C

Luau e Janet (clonados e lidos em `~/aipo-refs`), notas docs-only de
Tcl/PowerShell/Nushell (**sem clone** — só o que interessa ao host), e
a tabela dos runtimes wasm. Pinagem em `~/aipo-refs/refs.lock.json`.

> Desvios de layout registrados:
> - Luau: `Compiler/`, `Analysis/`, `VM/` contêm `include/` + `src/`
>   (ex.: `VM/src/lbuffer.h`, não `VM/lbuffer.h`).
> - Janet: núcleo em `src/core/*.c` + API em `src/include/janet.h`
>   (`vm.c`, `gc.c`, `fiber.c`, `bytecode.c` em `src/core/`).
> - WAMR: interpretador em `core/iwasm/interpreter/`
>   (`wasm_interp_classic.c` / `wasm_interp_fast.c`); docs em `doc/`.
> - wasmi: crates em `crates/` (`wasmi`, `wasi`, `cli`, `core`, `ir`, …).

---

## 1. Luau (`luau-lang/luau`, `master`)

### 1.1 Typing gradual: contratos, não coerção

O `Analysis/` implementa inferência por restrições
(`ConstraintGenerator`, `ConstraintSolver` em
`Analysis/src/` — listados via `ls`), com fronteira pública em
`Analysis/include/Luau/Frontend.h`: `struct Frontend`
(`Frontend.h:163`), `check(name)` retornando `CheckResult`
(`Frontend.h:199`), e o catálogo de erros em
`Analysis/include/Luau/Error.h` (`TypeMismatch`, `TypeErrorData =
Variant<…>`, `Error.h:32-43,668`, `TypeErrorSummary`, `Error.h:740`).

Modelo para a Aipo: **tipos opcionais como contratos verificados pelo
frontend** (`campo: Tipo` ⇒ erro diagnóstico, nunca coerção
silenciosa). A separação `Frontend::check` → diagnósticos é o padrão a
copiar para o nosso `aipo-compiler` (máx. 8 arquivos lidos neste
estudo: `Compiler/src/{Compiler,BuiltinFolding,ConstantFolding,
CostModel,TableShape,ValueTracking}.cpp`, `VM/src/lbuffer.h`,
`Analysis/include/Luau/{Frontend,Error}.h`).

### 1.2 `buffer` ≈ `Bytes`

`VM/src/lbuffer.h:1-13`: limite `MAX_BUFFER_SIZE (1 << 30)`
(`lbuffer.h:9`), layout `offsetof(Buffer, data) + max(len, 8)`
(`lbuffer.h:10`), construtor/destrutor
`luaB_newbuffer / luaB_freebuffer` (`lbuffer.h:12-13`), com
implementação em `VM/src/lbuffer.cpp` + stdlib em `VM/src/lbuflib.cpp`.
É um bloco GC de bytes com API de leitura/escrita — o análogo direto
do nosso `Bytes`.

### 1.3 Compilador com dobras explícitas

`Compiler/src/` contém passes nomeados: `ConstantFolding`,
`BuiltinFolding`, `ValueTracking`, `TableShape`, `CostModel`
(listados via `ls`), com entrada `compile(...)`
(`Compiler/include/Luau/Compiler.h:100`). Lição: otimizações como
passes pequenos e testáveis, não como flag monolítica.

### Lição → Ação na Aipo

- **Contratos de tipo**: `campo: Tipo` verificado no frontend com
  diagnósticos estilo `Error.h` (variante tipada + sumário), sem mudar
  semântica de runtime. Crates: `aipo-compiler` (diagnósticos),
  `aipo-vm` (`AssertContract`). Vira ADP/issue.
- **`Bytes`**: conferir limites e layout contra `lbuffer.h:9-13`
  (teto 1 GiB, mínimo 8 bytes reservados). Crates: `aipo-vm`,
  `aipo-stdlib`.
- **Passes de dobra**: `ConstantFolding`/`BuiltinFolding` como modelo
  de passes incrementais no emissor. Crates: `aipo-compiler`.

---

## 2. Janet (`janet-lang/janet`, `master`)

### 2.1 `Janet`: tagged union em 3 modos

(`~/aipo-refs/janet/src/include/janet.h:518-592`): 17 tipos
(`JANET_NUMBER … JANET_POINTER`, `janet.h:519-537`); representação
tripla — `JANET_NANBOX_64` (`u64/i64/number/pointer`, `janet.h:540-546`),
`JANET_NANBOX_32` (payload + tag u32, `janet.h:547-566`), fallback
struct `{ as; type; }` (`janet.h:568-577`). Verificação via
`janet_checktype`/`janet_type` (`janet.h:803-876`).

Núcleo C enxuto: `src/core/` soma **5161 linhas** em 4 arquivos
(`vm.c` 1763 + `bytecode.c` 1931 + `gc.c` 779 + `fiber.c` 688,
contados por `wc -l`). Prova de que VM+GC+fibers cabem em ~5k linhas.

### 2.2 VM: `goto` computado (GCC/clang) com fallback `switch`

(`~/aipo-refs/janet/src/core/vm.c:54-68`): `VM_START` =
`goto *op_lookup[first_opcode]` (`vm.c:61`), `vm_next()` =
`goto *op_lookup[*pc & 0xFF]` (`vm.c:65`); senão
`switch(opcode)` em `for(;;)` (`vm.c:68`). Terceira confirmação
independente (com Wren e LuaJIT) de que **o degrau `switch` → `goto`
computado é mecânico e reversível**.

### 2.3 GC: mark por tipo; fibers redimensionáveis

- Marcação: um `janet_mark_*` por tipo
  (`gc.c:35-45`: funcenv, funcdef, function, array, table, struct,
  tuple, buffer, string, fiber, abstract).
- Fibers: `janet_fiber_reset` / `janet_fiber` / `janet_fiber_push`
  (`fiber.c:48-136`); pilha cresce por `janet_fiber_grow`
  (`fiber.c:130-132`) via `setcapacity` (`fiber.c:117`).

### Lição → Ação na Aipo

- **Valor em 3 modos**: o trio nanbox64/nanbox32/struct
  (`janet.h:540-577`) é o modelo se a Aipo precisar de builds 32-bit
  (perfil nano) sem bifurcar o código. Crates: `aipo-vm`. Vira
  ADP/issue.
- **Orçamento de núcleo**: ~5k linhas para VM+GC+fibers
  (`vm.c+gc.c+fiber.c+bytecode.c`) como teto de complexidade do
  `aipo-vm`. Crates: `aipo-vm`.
- **Fibers**: `fiber.c:48-136` como especificação mínima do nosso
  `Task` (reset/push/grow). Crates: `aipo-vm`.

---

## 3. Notas docs-only (sem clone)

Tcl, PowerShell e Nushell **não foram clonados** — entram só pelos
dois pontos que calibram o host Aipo. Sem citações de arquivo (não
lidos); validar antes de virar ADP.

- **Tcl (reentrância do host)**: o interpretador Tcl é historicamente
  reentrante por desenho (eval aninhado, `Tcl_Eval` recursivo seguro).
  Ponto para o host AHS: chamadas host→Aipo→host devem ter profundidade
  limitada e estado de erro por invocação, não global.
- **PowerShell/Nushell (pipelines de valores estruturados)**: cmdlets
  trocam objetos tipados, não texto. Ponto para a Aipo: stdlib de
  `task`/stream deve transportar `Value` tipado entre estágios, com
  contrato de forma (shape) verificável — converge com QuickJS shapes
  (§4 de `reference-vms.md`).

### Lição → Ação na Aipo

- Host reentrante com orçamento de profundidade + erro por invocação.
  Crates: `aipo-host`, `aipo-vm`. Vira ADP/issue.
- Pipelines tipados fim-a-fim. Crates: `aipo-stdlib`, `aipo-vm`.

---

## 4. Runtimes wasm — tabela comparativa

| Runtime | Tamanho / perfil | WASI | Configuração relevante | Papel na Aipo |
|---|---|---|---|---|
| `wasmtime` (referência externa, sem clone) | ~25 MB (full-only) | sim | — | Teto full; nunca no nano/shell |
| `wasm3` (`main`) | **~64 KB código / ~10 KB RAM** (`README.md:43`); pilha de função `d_m3MaxFunctionStackHeight 8000` (`source/m3_config.h:19-20`); perfil determinístico documentado (`source/m3_config.h:162-185`, `source/m3_deterministic.h:1-8`) — relógio fixo, `random_get` com seed fixa | via `source/m3_api_wasi.c` (presente na árvore) | `source/m3_config.h` inteiro é o painel de flags | Modelo de perfil determinístico; candidato nano se WASI bastar |
| `WAMR` (`main`) | pequeno footprint, multiplataforma (`README.md:30`); RISC-V 32/64 suportado (`README.md:76`); dados de footprint no wiki (`README.md:105`) | **sim** | `WAMR_BUILD_TARGET` (X86_64/ARM/RISCV32/… em `build-scripts/config_common.cmake:4-46`); `WAMR_BUILD_FAST_INTERP` alterna `wasm_interp_fast.c` ↔ `wasm_interp_classic.c` (`core/iwasm/interpreter/iwasm_interp.cmake:10-13`); `WAMR_BUILD_MINI_LOADER` (`iwasm_interp.cmake:16`) | **Candidato líder P3**: WASI + configurável + RISC-V32 |
| `wasmi` (`main`) | interpretador Rust puro, `no_std` embarcado, fuel metering, 100% spec testsuite, auditorias 2023/2024 (`README.md:20-43`); **possui `crates/wasi`** com suporte `wasip1` (`README.md:73`, `crates/wasi`) | **sim (`wasmi_wasi`, `wasip1`)** — corrige a hipótese inicial "sem WASI" | features da crate `wasmi` (`docs`: `README.md:57-59`) | Alternativa Rust-nativa; decidir WAMR vs wasmi no spike P3 com critério WASI + `no_std` + metering |

Nota de honestidade: os números "~64 KB", "95 KB RISC-V32" e "25 MB"
vêm de READMEs/wikis a montante e variam por flags/target — o spike P3
deve medir os três candidatos com as mesmas flags (`MINI_LOADER`,
`FAST_INTERP`, determinístico) antes de qualquer ADP.

### Lição → Ação na Aipo

- P3: spike WAMR (líder) vs wasm3 (menor) vs wasmi (Rust-nativo),
  todos com WASI ligado, medindo KB + suporte a `RISCV32` +
  determinismo. Crates: `aipo-wasm` (futura). Vira ADP/issue.
- Perfil determinístico de wasm3 (`m3_config.h:162-185`) como modelo
  do modo replay da Aipo. Crates: `aipo-vm`.
