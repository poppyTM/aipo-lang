# VMs de Referência — Tier A

Estudos profundos de Lua 5.4 (`lua/lua`), LuaJIT (`v2.1`), Wren e
QuickJS-ng. Metodologia: leitura direta dos clones em `~/aipo-refs`
(pinados em `~/aipo-refs/refs.lock.json`). Toda afirmação factual cita
arquivo + linha lidos.

> Desvio de layout registrado: o repo `lua/lua` mantém os fontes na
> **raiz** (`~/aipo-refs/lua/lobject.h`, não `src/lobject.h`).
> Os caminhos abaixo já refletem o layout real.

---

## 1. Lua 5.4 (`lua/lua`, `master`)

### 1.1 `TValue`: tamanho e layout em 64-bit

O valor base é uma união + tag (`~/aipo-refs/lua/lobject.h:49-69`):

- `union Value` (`lobject.h:49-57`): ponteiro GC (`gc`), `void *p`,
  `lua_CFunction f`, `lua_Integer i`, `lua_Number n` — 8 bytes em 64-bit.
- `struct TValue` (`lobject.h:67-69`): `Value value_; lu_byte tt_` —
  8 bytes de payload + 1 byte de tag (+ padding → **16 bytes** em LP64).
- Tags usam bits 0-3 + variantes 4-5 + bit 6 "collectable"
  (`lobject.h:34-42`); cópia é `memcpy` da união + tag
  (`setobj`, `lobject.h:118-121`).

Resposta: `TValue` = 16 bytes em 64-bit (8 payload + 1 tag + padding).
A RegVm pode mirar o mesmo envelope (payload u64 + tag u8/u32).

### 1.2 Upvalues abertas: lista ordenada + `luaF_close`

- `luaF_findupval` (`~/aipo-refs/lua/lfunc.c:87-99`): percorre a lista
  `L->openupval` **ordenada por nível de pilha** (`uplevel(p) >= level`,
  `lfunc.c:91`); retorna a existente se `uplevel(p) == level`
  (`lfunc.c:93-94`), senão cria via `newupval` inserindo após `pp`
  (`lfunc.c:98`).
- Custo pior caso de `findupval`: **O(n)** no número de upvalues
  abertas (varredura linear da lista) — segue direto do `while` em
  `lfunc.c:91-96`.
- `luaF_closeupval` (`lfunc.c:197-210`): desvincula (`luaF_unlinkupval`,
  `lfunc.c:202`), copia o valor para o slot próprio
  (`setobj(L, slot, uv->v.p)`, `lfunc.c:203`) e reponta
  (`uv->v.p = slot`, `lfunc.c:204`) — exatamente o desenho citado na
  semente (`findupval` ordenado + `close` copia-para-dentro-e-reponta).
- `MAXUPVAL 255` (`~/aipo-refs/lua/lfunc.h:29`): cabe num campo `u8`;
  **cabe folgado no nosso formato u32** (8 bits de índice + resto livre).

### 1.3 Dispatch em `luaV_execute`: `switch`, não `goto` computado

- `vmdispatch(o)` = `switch(o)`, `vmcase(l)` = `case l:`,
  `vmbreak` = `break` (`~/aipo-refs/lua/lvm.c:1199-1201`).
- Laço principal `luaV_execute (lua_State *L, CallInfo *ci)`
  (`lvm.c:1204`); registradores via macro `RA(i)` = `base+GETARG_A(i)`
  (`lvm.c:1108`), com `updatebase(ci)` = `base = ci->func.p + 1`
  (`lvm.c:1122`) — **janela de registradores por base de frame**,
  sem copiar o banco inteiro por chamada.
- Formato da instrução: u32 com opcode nos **primeiros 7 bits** e
  formatos `iABC / ivABC / iABx / iAsBx / iAx / isJ`
  (`~/aipo-refs/lua/lopcodes.h:15-26`); `SIZE_OP 7`, `POS_OP 0`
  (`lopcodes.h:51-53`).
- Opcodes de upvalue/closure: `OP_GETUPVAL` / `OP_SETUPVAL`
  (`lopcodes.h:244-245`), `OP_CLOSURE` (`lopcodes.h:337`),
  tratados em `lvm.c:1293-1299` e `lvm.c:1935`.

### Lição → Ação na Aipo

- **Upvalues**: implementar `MakeClosure/GetUpvalue/SetUpvalue` com a
  lista ordenada de Lua (`lfunc.c:87-99`) em vez de busca ingênua;
  `close` copia-valor-e-reponta (`lfunc.c:203-204`).
  Crates afetadas: `aipo-vm` (RegVm), `aipo-compiler` (emissão de
  `OP_CLOSURE`-equivalente). Vira ADP/issue antes do código.
- **Janela de registradores**: trocar a cópia de 256 registradores por
  chamada (`crates/aipo-vm/src/reg_vm.rs:122-123`) por base+slots à la
  `updatebase` (`lvm.c:1122`). Crates: `aipo-vm`.
- **Formato u32**: reservar 7-8 bits de opcode + `A/B/C` como
  `lopcodes.h:15-26`; índice de upvalue em 8 bits (`MAXUPVAL=255`
  cabe). Crates: `aipo-compiler`, `aipo-vm`.

---

## 2. LuaJIT (`LuaJIT/LuaJIT`, `v2.1`)

### 2.1 `TValue`: 8 bytes, NaN-tagging / tagged union

`typedef LJ_ALIGN(8) union TValue` (`~/aipo-refs/luajit/src/lj_obj.h:174-213`):

- Sobrepõe `u64` / `lua_Number` / `GCRef`+tag (`lj_obj.h:174-196`);
- sem GC64: `GCRef gcr | int32 i` no LSB + `uint32 it` (tag) no MSW
  sobreposto ao MSW de um `double` (`lj_obj.h:187-196`) — NaN-tagging
  documentado em `lj_obj.h:224-240` (MSWs `> 0xfff80000` livres como tags).
- Com GC64: tag de 4 bits dentro do NaN quiet + 47 bits de payload
  (`lj_obj.h:242-249`).

Resposta: `TValue` LuaJIT = **8 bytes** (metade de Lua 5.4). Preço:
acoplamento ao `double` IEEE-754 e dois layouts (`LJ_GC64` on/off).

### 2.2 Upvalues: mesma lista ordenada, com `closed` explícito

- `func_finduv` (`~/aipo-refs/luajit/src/lj_func.c:36-69`): mesma busca
  ordenada (`uvval(p) >= slot`, `lj_func.c:44`); ressuscita se morta
  (`lj_func.c:47-48`); insere ordenado (`lj_func.c:60-61`).
- `lj_func_closeuv` (`lj_func.c:82-100`): fecha tudo `>= level`
  (`lj_func.c:87-88`); se morta, libera (`lj_func.c:93-94`), senão
  desvincula + `lj_gc_closeuv` (`lj_func.c:96-97`).
- Custo open/close de frame: O(upvalues do frame), não O(registradores)
  — o frame LuaJIT é **base + slots na pilha**, sem `memcpy` do banco.

### 2.3 Bytecode: palavra de 32 bits, `ABC`/`AD`

Formato (`~/aipo-refs/luajit/src/lj_bc.h:14-27`): 32 bits, `| B | C | A | OP |`
(ABC) ou `| D | A | OP |` (AD); limites `BCMAX_A/B/C = 0xff`,
`BCMAX_D = 0xffff`, `BCBIAS_J = 0x8000` (`lj_bc.h:25-29`); acesso via
`bc_op/bc_a/bc_b/bc_c/bc_d/bc_j` (`lj_bc.h:34-39`).

### 2.4 Dispatch: assembly gerado (DynASM), não `switch` C

O interpretador é gerado a partir de `vm_*.dasc`
(e.g. `src/vm_x64.dasc`, `src/vm_arm64.dasc`); o `vm_arm64.dasc`
documenta duas estratégias: *replicated dispatch* vs *common dispatch*
(`~/aipo-refs/luajit/src/vm_arm64.dasc:241-245`) com continuação via
`->cont_dispatch` / `->vm_call_dispatch`
(`vm_arm64.dasc:371,581-586,617-619`). Ou seja: **threaded code em
assembly**, ordens de magnitude acima do `switch` de `lvm.c` — mas ao
custo de um backend por arquitetura (inviável para a fase atual da Aipo).

### Lição → Ação na Aipo

- **Não copiar LuaJIT agora**: o ganho vem de assembly gerado por
  arquitetura. Nossa escada realista: `switch` (hoje, como Lua 5.4)
  → `goto` computado (Wren, §3) → DynASM só se o perfil justificar.
  Crates: `aipo-vm`. Vira ADP/issue.
- **Upvalues**: reutilizar `func_finduv`/`lj_func_closeuv`
  (`lj_func.c:36-100`) como especificação do `close` da RegVm.
  Crates: `aipo-vm`.
- **TValue de 8 bytes**: avaliar NaN-tagging como otimização futura;
  hoje manter envelope explícito (debugabilidade > densidade).
  Crates: `aipo-vm`. Vira ADP/issue.

---

## 3. Wren (`wren-lang/wren`, `main`)

### 3.1 Tamanho: pequeno de verdade

`src/vm/wren_vm.c` tem **1994 linhas**; o conjunto
`src/vm/* + src/include/*` soma **13138 linhas** com **3721 `;`**
(contados por `grep -o ";" | wc -l` sobre os fontes). A tese "VM < 4k
`;`" refere-se ao núcleo interpretador — `wren_vm.c` sozinho tem
apenas **691 `;`**. Ponto confirmado: o interpretador cabe numa leitura.

### 3.2 Dispatch: `goto` computado com fallback `switch`

(`~/aipo-refs/wren/src/vm/wren_vm.c:890-918`): com `WREN_COMPUTED_GOTO`,
tabela `dispatchTable[]` (`wren_vm.c:892-896`) + `goto *dispatchTable[...]`
(`wren_vm.c:905`); sem ele, `switch` clássico (`wren_vm.c:910-916`).
É o degrau intermediário entre Lua (`switch`) e LuaJIT (assembly):
**mesmo laço, duas estratégias, troca por `#define`**. Modelo ideal
para a RegVm medir antes de decidir.

### 3.3 Compilador single-pass: `emitJump` + `patchJump`

- `emitJump` (`~/aipo-refs/wren/src/vm/wren_compiler.c:1362-1367`):
  emite opcode + placeholder `0xff 0xff`, retorna o índice.
- `patchJump` (`wren_compiler.c:1756-1766`): calcula
  `jump = code.count - offset - 2` (`wren_compiler.c:1759`), erra se
  `jump > MAX_JUMP` (`1 << 16`, `wren_compiler.c:38,1760`), escreve
  dois bytes big-endian (`wren_compiler.c:1762-1763`).
- Uso canônico em `and/or/if/else` (`wren_compiler.c:2547-2586`) e
  saída de laço (`wren_compiler.c:2982-3007`).

Resposta: forward jumps sem IR — placeholder de 16 bits + patch no
fim do bloco. Limite: **64 Ki instruções por salto**.

### 3.4 Fibers como primitiva; upvalues por fiber

- `ObjFiber` (`~/aipo-refs/wren/src/vm/wren_value.h:316-355`): pilha
  heap (`stack/stackTop/stackCapacity`), frames dinâmicos
  (`frames/numFrames/frameCapacity`), **lista própria de upvalues
  abertas** (`openUpvalues`), `caller` para `yield` retomar
  (`wren_value.h:348`), `error` e `state`.
- `ObjUpvalue` (`wren_value.h:178-197`): `value` aponta para a pilha;
  ao fechar, o valor é içado para `closed` e `value` reponta para lá
  (`wren_value.h:186-189`) — o mesmo desenho Lua, mas **por fiber**.
- API C: `wrenCall` executa método num fiber novo
  (`~/aipo-refs/wren/src/include/wren.h:322-347`); `wrenAbortFiber`
  (`wren.h:544-546`).

Métodos: dispatch é por **classes + assinatura**, sem inline caches
(nenhum `cache` no `wren_vm.c` — verificado por `grep`; dispatch por
tabela de métodos da classe, não por call-site). Para a Aipo: simples
e suficiente até o profiler dizer o contrário.

### Lição → Ação na Aipo

- **Dispatch em 2 modos**: copiar o padrão `WREN_COMPUTED_GOTO`
  (`wren_vm.c:890-918`) — `switch` por padrão, `goto` computado por
  feature/flag, medido no nosso benchmark. Crates: `aipo-vm`.
  Vira ADP/issue.
- **Fibers vs `Task`**: nosso scheduler pode espelhar `ObjFiber`
  (pilha + frames + `caller`, `wren_value.h:316-355`) em vez de pilha
  do SO por tarefa. Crates: `aipo-vm`, `aipo-task` (quando existir).
- **Single-pass como referência do frontend**: `emitJump/patchJump`
  (`wren_compiler.c:1362-1367,1756-1766`) é o modelo se o compilador
  Aipo quiser saltos sem IR; limite 64 Ki documentado. Crates:
  `aipo-compiler`.

---

## 4. QuickJS-ng (`quickjs-ng/quickjs`, `master`)

> Desvio de layout registrado: `quickjs.c` + `quickjs-opcode.h` na
> **raiz** (confirmado); `docs/developer-guide/internals.md` **não
> existe** — `docs/` é um site docusaurus
> (`docs/{babel.config.js,docusaurus.config.js,src,static}`). O estudo
> abaixo lê o fonte, não o doc ausente.

### 4.1 `JSValue`: largura depende do modo

(`~/aipo-refs/quickjs/quickjs.h:173-300`): três representações por
`#ifdef`:

- `JS_NAN_BOXING` (padrão 64-bit, `quickjs.h:251-253`):
  `typedef uint64_t JSValue` — **8 bytes**, tag nos 32 bits altos
  (`JS_VALUE_GET_TAG`, `quickjs.h:255`), payload embaixo; floats via
  deslocamento `JS_FLOAT64_TAG_ADDEND` (`quickjs.h:264-280`).
- Sem NaN-boxing / `JS_CHECK_JSVALUE`: `struct JSValue*` opaco
  (`quickjs.h:225`) — ponteiro com tag nos 4 bits baixos.
- Tags internas negativas para GC-objetos (`JS_TAG_OBJECT = -1`,
  `JS_TAG_STRING = -7`, …, `quickjs.h:181-198`).

Resposta: 8 bytes no modo padrão 64-bit; 16 bytes nas variantes struct.

### 4.2 GC: refcount + removedor de ciclos (sem raízes explícitas)

- Cada objeto carrega `ref_count` + `mark` no cabeçalho do alocador
  (`js_rc(p)`, `~/aipo-refs/quickjs/quickjs.c:284-296,499-509`);
  fase global em `JSRuntime.gc_phase` (`quickjs.c:362`).
- `JS_RunGC` (`quickjs.c:7484-7493`): `gc_decref` → `gc_scan` →
  `gc_free_cycles`. `gc_decref` (`quickjs.c:7374-7394`) decrementa os
  filhos de todos os GC-objetos e isola os de contagem zero em
  `tmp_obj_list`; `gc_scan` (`quickjs.c:7413-7431`) re-soma o que é
  alcançável de contadores > 0; `gc_free_cycles` (`quickjs.c:7433-7482`)
  libera o resto sob `JS_GC_PHASE_REMOVE_CYCLES` (`quickjs.c:7441`).
- Disparo: **limiar de malloc** — `malloc_gc_threshold = 256 KiB`
  inicial (`quickjs.c:2334`); ao ultrapassar,
  `JS_RunGC` + novo limiar = `malloc_size + threshold`
  (`quickjs.c:1687-1696`); ajustável por `JS_SetGCThreshold`
  (`quickjs.c:2462-2464`).

Resposta: o ciclo removedor dispara por **pressão de alocação**, não
por temporizador; usa só refcounts + travessia (sem conjunto de raízes
separado — a "raiz" é todo objeto com `ref_count > 0`).

### 4.3 Shapes (propriedades compartilhadas) e átomos (strings internadas)

- `JSShape` (`quickjs.c:1108-1120`): protótipo + nomes/flags
  compartilhados; cada objeto aponta para `shape`
  (`quickjs.c:1140`); hash global em
  `JSRuntime.shape_hash` (`quickjs.c:418-421`); formas iniciais por
  classe (`array_shape`, `regexp_shape`, … `quickjs.c:574-578`).
- Átomos: tabela hash `atom_hash` no runtime
  (`quickjs.c:343-347`); `__JS_NewAtomInit` (`quickjs.c:1267`);
  inteiros pequenos viram átomos etiquetados
  (`__JS_AtomFromUInt32`, `quickjs.c:3165`).

### 4.4 Bytecode sem IR: formatos `FMT`

`quickjs-opcode.h` declara formatos de operando (`FMT(u8)`,
`FMT(const)`, `FMT(atom)`, … — `quickjs-opcode.h:32-51`) consumidos
direto pelo emissor; não há IR intermediário — o parser emite
bytecode com esses formatos.

### Lição → Ação na Aipo

- **Shapes vs `struct_fixed_fields`**: o `shape` compartilhado
  (`quickjs.c:1108-1140`) é o modelo para selar structs — transições
  de shape viram erro amigável em vez de dicionário silencioso.
  Crates: `aipo-vm` (`SealStruct`), `aipo-compiler`. Vira ADP/issue.
- **Átomos vs interning de nomes de campo**: internar nomes uma vez
  (`atom_hash`, `quickjs.c:343-347`) e comparar por id — mede-se
  contra o interning atual. Crates: `aipo-vm`, `aipo-compiler`.
- **GC por limiar**: se a Aipo adotar ciclo-coleta, disparar por
  `malloc_size` (`quickjs.c:1687-1696,2334`) é o padrão mais simples
  e determinístico. Crates: `aipo-vm`. Vira ADP/issue.
