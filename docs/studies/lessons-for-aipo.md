# Lições para a Aipo — síntese acionável

Síntese dos estudos (`reference-vms.md`, `reference-runtimes.md`).
**Nada aqui é decisão**: cada item vira ADP/issue antes de virar
código. Referências `§` apontam para as seções de estudo.

---

## 1. Lacunas da RegVm (gap list)

| Gap na RegVm | Estudo que informa | Ação proposta |
|---|---|---|
| `MakeClosure` | Lua `OP_CLOSURE` (`reference-vms.md §1.3`); `lvm.c:1935` | Closure captura upvalues pela lista ordenada, não por cópia plana |
| `GetUpvalue` / `SetUpvalue` | Lua `luaF_findupval` (`lfunc.c:87-99`); LuaJIT `func_finduv` (`lj_func.c:36-69`) | Busca O(n) em lista ordenada + `MAXUPVAL=255` em 8 bits |
| `FillSelfCapture` | Wren `ObjUpvalue.closed` (`reference-vms.md §3.4`) | Ao fechar, içar valor para slot próprio e repontar |
| `Await` | Wren `ObjFiber.caller` (`wren_value.h:348`); Janet `fiber.c:48-136` | Suspensão como troca de fiber com `caller`, não pilha do SO |
| `SealStruct` | QuickJS `JSShape` (`quickjs.c:1108-1140`) | Shape compartilhado; transição vira erro amigável |
| `AssertInvariant` | Luau `Frontend::check` (`reference-runtimes.md §1.1`) | Invariante como diagnóstico de frontend, não `panic` interno |
| `AssertContract` | Luau `Error.h:32-43,668` | Contratos `campo: Tipo` com erro tipado + sumário |
| `CheckMutations` | QuickJS `gc_phase` (`quickjs.c:362`) | Fase/modo explícito durante iteração mutável |
| `IterGuardEnd` | Wren `patchJump` de saída de laço (`wren_compiler.c:2982-3007`) | Guarda de iteração com salto corrigido e limite documentado |
| Gargalo cópia-de-registradores | Lua `updatebase` (`lvm.c:1122`); `crates/aipo-vm/src/reg_vm.rs:122-123` | Trocar `mem::replace` + `from_fn` de 256 `Value` por chamada por janela base+slots |

Implementações atuais desses opcodes vivem em
`crates/aipo-vm/src/vm/dispatch.rs:928-1274`
(`SealStruct`, `AssertInvariant`, `AssertContract`, `CheckMutations`,
`MakeClosure`, `GetUpvalue`, `SetUpvalue`, `IterGuardEnd`,
`FillSelfCapture`) — o estudo acima diz **como** devem evoluir, não
que estão ausentes.

## 2. Piso de dispatch (candidatos)

- Alvo de trabalho: **~100 ns/opcode** (estimativa de ordem de
  grandeza para calibrar benchmarks, **não** medida neste estudo).
- Escada verificada nos fontes: `switch` (Lua, `lvm.c:1199-1204`) →
  `goto` computado com fallback (Wren, `wren_vm.c:890-918`; Janet,
  `vm.c:54-68`) → assembly gerado (LuaJIT, `vm_*.dasc`).
- Recomendação: RegVm começa no `switch`, adiciona `goto` computado
  por feature/flag como Wren, mede; DynASM só com evidência do profiler.

## 3. P2 — matriz de perfis (nano/shell/full)

| Perfil | Agora | Depois | Regra de desenho |
|---|---|---|---|
| `nano` | Linux pequeno+rápido | MCU | `no_std` desde o início; trio de representação como Janet (`janet.h:540-577`) |
| `shell` | CLI interativo | — | `goto` computado opcional; GC por limiar (QuickJS, `quickjs.c:1687-1696`) |
| `full` | wasmtime permitido | — | Otimizações agressivas isoladas por feature |

## 4. P3 — substrato wasm

| Candidato | Estado neste estudo |
|---|---|
| `wasmtime` | Full-only (~25 MB); nunca no nano/shell |
| `WAMR` | **Candidato líder**: WASI + `RISCV32` + flags (`FAST_INTERP`, `MINI_LOADER`) — `reference-runtimes.md §4` |
| `wasm3` | Menor (~64 KB / 10 KB RAM) + perfil determinístico como modelo de replay |
| `wasmi` | Alternativa Rust-nativa com `wasmi_wasi` (`wasip1`); decidir no spike |

Spike P3 mede os três com WASI ligado antes de qualquer ADP
(critérios: KB, `RISCV32`, determinismo, `no_std`, metering).

## 5. P4 — regras anti-fragmentação

1. **Uma semântica / N emissores**: estudos nunca criam dialeto; só
   ADP muda semântica.
2. **Tier 1/2/3 com erros amigáveis**: Tier 1 = núcleo; Tier 2/3 =
   recusas com mensagem acionável (modelo Luau `Error.h`), nunca
   silencioso.
3. **Corpus de conformance como árbitro**: divergência entre perfis =
   bug no corpus, não "comportamento de perfil".
4. **Perfis via `features` + `[profile.*]`, nunca crates novas**:
   nano/shell/full são combinações de flags, não forks.
5. **Matriz CI `full/shell/nano`**: todo PR roda os três perfis; falha
   num perfil bloqueia merge.
