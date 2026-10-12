# VMs de Referência — Tier A

**Rodada:** 2026-10-09. Fontes: [manifest](../../studies/refs.json). Os checkouts foram obtidos em commits completos com HEAD e origin conferidos, lidos fora do repo via `./studies/fetch-refs.sh`. Esta nota registra leitura dirigida de código, sem compilar ou transformar números de terceiros em medidas da nossa implementação. Toda afirmação factual cita arquivo + linha lidos.

> Desvios de layout registrados:
> - O repo `lua/lua` mantém os fontes na **raiz** (`lobject.h`, não `src/lobject.h`).
> - `quickjs.c` + `quickjs-opcode.h` estão na **raiz**; `docs/` é um site docusaurus, sem guia de internals em markdown.

---

## 1. Wren (`wren-lang/wren`, `main`)

**O que é:** linguagem com VM compacta e embedding C. A arquitetura de fibers, frames e upvalues fornece uma referência concreta para lifetimes de execução.

**Como funciona:**
- Em [`wren_vm.c`, `captureUpvalue`/`closeUpvalues`](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_vm.c#L244), upvalues abertos ficam em uma lista ordenada pelo slot. Fechar um upvalue copia o valor para armazenamento próprio e muda seu apontamento. O retorno fecha upvalues do frame antes de descartar slots.
- [`CallFrame`](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_value.h#L284) guarda IP, closure e base de stack. A VM atualiza caches locais de frame quando muda a ativação.
- [`WREN_COMPUTED_GOTO`](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_vm.c#L890) escolhe dispatch por labels no compilador compatível; existe fallback com `switch`.

### Detalhes técnicos verificados

- **Tamanho do núcleo:** `src/vm/wren_vm.c` tem ~1994 linhas e 691 `;`; o conjunto `src/vm/* + src/include/*` soma ~13k linhas com 3721 `;`. O interpretador cabe em uma leitura focada.
- **Dispatch em 2 modos:** com `WREN_COMPUTED_GOTO` (`wren_vm.c:890-918`), tabela `dispatchTable[]` (`wren_vm.c:892-896`) + `goto *dispatchTable[...]` (`wren_vm.c:905`); sem ele, `switch` clássico (`wren_vm.c:910-916`). É o degrau intermediário entre `switch` puro e assembly gerado: mesmo laço, troca por `#define`.
- **Compilador single-pass:** `emitJump` (`wren_compiler.c:1362-1367`) emite opcode + placeholder de 16 bits; `patchJump` (`wren_compiler.c:1756-1766`) calcula o delta e valida `jump <= MAX_JUMP (1 << 16)`. Forward jumps sem IR com limite de 64 Ki instruções por salto.
- **Fibers como primitiva:** `ObjFiber` (`wren_value.h:316-355`) mantém pilha heap, frames dinâmicos, lista própria de upvalues abertas, e ponteiro `caller` para retomar após `yield` (`wren_value.h:348`). `ObjUpvalue` (`wren_value.h:178-197`) iça o valor para `closed` ao fechar.
- **Dispatch de métodos:** por classes + assinatura, sem inline caches no interpretador central.

**Perguntas fechadas:** captura exige uma relação explícita entre slot e lifetime; retorno precisa fechar capturas antes de invalidar slots; computed goto é uma escolha condicionada de build, não uma característica que se copia literalmente para Rust seguro.

**Lição → Ação na Aipo:**
- RegVM isola handlers e restaura estado do caller; próxima etapa: modelar frames com base/slots e células de captura, mantendo a semântica do canon.
- **Dispatch em 2 modos:** padrão `switch`, com `goto` computado por feature/flag como experimento futuro pareado em benchmark. Crates: `aipo-vm`.
- **Fibers vs `Task`:** scheduler pode espelhar `ObjFiber` (pilha + frames + `caller`) para suspensão cooperativa leve. Crates: `aipo-vm`.
- **Single-pass:** `emitJump/patchJump` é referência se o frontend emitir saltos sem IR. Crates: `aipo-compiler`.

---

## 2. QuickJS-ng (`quickjs-ng/quickjs`, `master`)

**O que é:** engine JavaScript embutível. Serve como referência para organização de valores, nomes de propriedades, ownership e coleta de ciclos, não como proposta de adotar a semântica de JavaScript.

**Como funciona:**
- [`quickjs.h`](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.h#L208) distingue ownership de `JSValue`/`JSValueConst`; há representações de valor condicionadas por macros, incluindo union+tag.
- [`JSShapeProperty`/`JSShape`](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.c#L1102) associam atoms, flags e uma tabela de propriedades.
- [`JS_RunGC`](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.c#L7484) é o ponto para estudar o coletor; sua política inteira não foi auditada nesta reconstrução.

### Detalhes técnicos verificados

- **`JSValue` e largura por modo:** sob `JS_NAN_BOXING` (`quickjs.h:251-280`), `uint64_t` (8 bytes) com tag nos 32 bits altos e floats via offset; sem NaN-boxing, struct ponteiro+tag (16 bytes). Tags negativas para objetos gerenciados (`JS_TAG_OBJECT = -1`, `JS_TAG_STRING = -7`).
- **GC por refcount + removedor de ciclos:** cada objeto carrega `ref_count` e `mark` (`quickjs.c:284-296`). `JS_RunGC` executa `gc_decref` (isolar contadores zerados) → `gc_scan` (re-somar alcançáveis) → `gc_free_cycles` (liberar ciclos sob `JS_GC_PHASE_REMOVE_CYCLES`). Dispara por pressão de alocação via limiar de malloc (`malloc_gc_threshold = 256 KiB`, `quickjs.c:1687-1696,2334`), sem conjunto de raízes separado.
- **Shapes e átomos:** `JSShape` (`quickjs.c:1108-1140`) compartilha protótipo e layout de nomes/flags entre instâncias com hash global `shape_hash`. Nomes são internados na tabela hash `atom_hash` (`quickjs.c:343-347`).
- **Bytecode sem IR:** formatos declarados em `quickjs-opcode.h` (`FMT(u8)`, `FMT(const)`, `FMT(atom)`) consumidos diretamente na emissão.

**Perguntas fechadas:** nomes internados e layouts compartilhados são mecanismos diferentes de valores da propriedade; representação compacta exige configuração e invariantes. Shapes não equivalem apenas a um `HashMap` adicionado a cada instância.

**Lição → Ação na Aipo:**
- Manter a semântica de `Value` centralizada e medir custo de field lookup antes de introduzir shapes/slots na Aipo.
- **Shapes vs `struct_fixed_fields`:** `shape` compartilhado é o modelo para selar structs (`SealStruct`) — transições inválidas viram erro diagnóstico amigável em vez de dicionário dinâmico. Crates: `aipo-vm`, `aipo-compiler`.
- **Átomos:** internar nomes uma vez e comparar por id (`atom_hash`). Crates: `aipo-vm`.
- **GC por limiar:** se houver coletor de ciclos, disparo por `malloc_size` é mais previsível que temporizadores. Crates: `aipo-vm`.

---

## 3. Lua (`lua/lua`, `master`)

**O que é:** runtime embutível com valores tagged, stack de slots e closures com upvalues.

**Como funciona:**
- [`lobject.h`](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/lobject.h#L49) define payload e tag de `TValue`.
- [`luaF_findupval`](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/lfunc.c#L85) percorre upvalues abertos por nível, reusa o existente ou cria outro. Uma lista ordenada ainda pode ter busca linear no número de capturas abertas percorridas.
- [`luaV_execute`](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/lvm.c#L1210) recebe o mecanismo de dispatch condicionado por `LUA_USE_JUMPTABLE`. O source estudado é o commit pinado do manifest.

### Detalhes técnicos verificados

- **`TValue`: tamanho e layout em 64-bit:** `union Value` (8 bytes: GC, ponteiro, cfunc, int, num) + `lu_byte tt_` em `struct TValue` (`lobject.h:49-69`), totalizando 16 bytes em LP64 com padding. Tags nos bits 0-3 + variantes 4-5 + flag collectable bit 6. Cópia via `memcpy` (`setobj`).
- **Upvalues abertas e lista ordenada:** `luaF_findupval` (`lfunc.c:87-99`) varre `L->openupval` ordenada por nível de pilha (`uplevel(p) >= level`). Custo pior caso O(n) no número de upvalues abertas. `luaF_closeupval` (`lfunc.c:197-210`) desvincula da lista, copia o valor para o slot do objeto upvalue (`setobj(L, slot, uv->v.p)`) e reponta (`uv->v.p = slot`). `MAXUPVAL 255` (`lfunc.h:29`) cabe em 8 bits.
- **Dispatch e janelas de registradores:** `vmdispatch` = `switch`, `vmcase` = `case`, `vmbreak` = `break` (`lvm.c:1199-1201`). Janela de registradores por base de frame: `RA(i)` = `base + GETARG_A(i)` com `updatebase(ci)` = `base = ci->func.p + 1` (`lvm.c:1108,1122`), sem copiar o banco inteiro de registradores em chamadas. Formato de instrução u32 com opcode nos primeiros 7 bits (`lopcodes.h:15-53`).

**Perguntas fechadas:** layout e custo de captura dependem da representação; a busca ordenada não vira O(1); o mecanismo de dispatch precisa ser analisado no alvo e build reais.

**Lição → Ação na Aipo:**
- Estudar slots e capturas antes de remover a troca de 256 registradores de RegVM. Nesta rodada, os operandos e a restauração do caller foram corrigidos; a migração para janelas base+slots permanece proposta.
- **Upvalues:** implementar `MakeClosure/GetUpvalue/SetUpvalue` usando lista ordenada por nível de stack em vez de cópias soltas; fechar upvalue içando valor para dentro da célula. Crates: `aipo-vm`, `aipo-compiler`.
- **Formato u32:** instrução de 32 bits com opcodes e operandos limitados; índices de upvalue cabem em 8 bits (`MAXUPVAL=255`). Crates: `aipo-compiler`, `aipo-vm`.

---

## 4. LuaJIT (`LuaJIT/LuaJIT`, `v2.1`)

**O que é:** implementação com interpretador, JIT e representação compacta de valores. É referência de encoding e frames, não dependência nova da Aipo.

**Como funciona:**
- [`lj_obj.h`](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_obj.h#L151) define `BCIns` como `uint32_t`, além de `TValue` e representações condicionadas por `LJ_GC64`/`LJ_FR2`.
- [`lj_bc.h`](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_bc.h#L25) fixa faixas e papéis ABC/AD/AJ.
- [`func_finduv`](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_func.c#L37) reusa células em lista ordenada de upvalues.

### Detalhes técnicos verificados

- **`TValue`: 8 bytes com NaN-tagging:** `union TValue` (`lj_obj.h:174-249`) sobrepõe `u64`, `lua_Number` e `GCRef`+tag. Em 32-bit/sem GC64, tag no MSW sobreposta a double NaN quiet; com GC64, tag de 4 bits dentro do NaN quiet e 47 bits de payload. Preço: acoplamento estrito ao padrão IEEE-754 e dois layouts dependentes de build.
- **Upvalues e frames:** `func_finduv` (`lj_func.c:36-69`) e `lj_func_closeuv` (`lj_func.c:82-100`) fecham células `>= level`. Frame na pilha gerenciado por base + slots, sem cópia plana do banco inteiro de registradores.
- **Bytecode de 32 bits:** formatos `ABC`, `AD`, `AJ` (`lj_bc.h:14-27`). Limites `BCMAX_A/B/C = 0xff`, `BCMAX_D = 0xffff`, `BCBIAS_J = 0x8000`.
- **Dispatch em assembly (DynASM):** o interpretador principal é gerado a partir de arquivos `.dasc` (`vm_x64.dasc`, `vm_arm64.dasc`), usando dispatch replicado ou comum com continuação por ponteiros (`vm_arm64.dasc:241-245,581-586`). Threaded code de alta performance, mas dependente de arquitetura específica de hardware.

**Perguntas fechadas:** largura da palavra não define por si a faixa de todos os operandos; formatos e papéis devem ser descritos por opcode. `GCRef` não deve ser anunciado como sempre 32 bits, pois existem configurações distintas.

**Lição → Ação na Aipo:**
- Documentar papéis de B, checar constantes e offsets e usar opcodes de iteração com C completo. NaN boxing e dispatch em assembly seguem experimentos futuros; não há justificativa atual para substituir o modelo seguro e portável de `Value`.
- **Escada de dispatch:** manter `switch` canônico na RegVm, avaliar `goto` computado (modelo Wren) com suporte condicional, e postergar geradores em assembly para quando houver perfil de produção que o demande. Crates: `aipo-vm`.
- **Upvalues:** reutilizar os invariantes de busca ordenada e fechamento explícito de `func_finduv`/`lj_func_closeuv`. Crates: `aipo-vm`.

---

## 5. Limites comuns e continuidade

A inspeção cobre as regiões citadas nos quatro runtimes Tier A. Uma análise integral de GC, compilação, FFI, reentrância e portabilidade exige rodadas dedicadas. Referências servem para formular hipóteses e invariantes estruturais, não para anunciar antecipadamente ganhos de performance, memória ou maturidade da Aipo.

Consulte [lições para Aipo](lessons-for-aipo.md) e o [guia de reimplementação](runtime-hardening-guide.md) para o mapeamento entre os fontes estudados e as mudanças efetivamente entregues no repositório.
