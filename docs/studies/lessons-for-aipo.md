# Lições para a Aipo — síntese acionável e plano de aplicação

Síntese dos estudos ([`reference-vms.md`](reference-vms.md), [`reference-runtimes.md`](reference-runtimes.md)).
**Status:** registro de estudo e plano de aplicação. As correções semânticas vêm do canon da Aipo. Referências externas ajudam a entender mecanismos; não têm autoridade para mudar a linguagem. Cada proposta vira ADP/issue antes de virar código.

---

## 1. O que virou implementação

| Lição | Referência | Aplicação entregue | Limite |
| --- | --- | --- | --- |
| Frames têm estado e lifetime explícitos | Wren, Lua, LuaJIT | Handlers isolados por ativação; caller restaurado em retorno/fault | Ainda salva 256 registradores por chamada |
| Campos de bytecode têm papéis específicos | LuaJIT | Checagem de B por opcode, jumps/pool/calls e iteração com C completo | Falta verificador integral Reg |
| Uma semântica não deve ser duplicada em emissores | Frontend compartilhado Aipo | Aritmética/igualdade Reg delegadas a Value; Dict corrigido VM/Reg/JS | Paridade geral continua incompleta |
| Contagens e buffers exigem limites | Luau, wasm3 | Bytes rejeita categorias inválidas; Reg tem budget; Wasm expõe fuel | Não cobre memória/output/tempo de host |
| Erros de engine precisam chegar ao embedding | Runtimes Wasm e contrato Rust | MissingExport e erros do writer propagados | Binding de string mantém silêncio em ponteiros inválidos |
| Layouts e ganhos dependem de alvo/flags | QuickJS, Janet, LuaJIT | Probe de layout e harness de medição reproduzível | Nenhum resultado quantitativo novo |
| Features do núcleo diferem de integração | wasmi, WAMR | Documentação corrige confusão no_std/WASI e mantém runner opcional | Nenhum runtime alternativo integrado |

## 2. Lacunas e roadmap da RegVm (gap list)

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

Implementações atuais desses opcodes vivem em `crates/aipo-vm/src/vm/dispatch.rs:928-1274` (`SealStruct`, `AssertInvariant`, `AssertContract`, `CheckMutations`, `MakeClosure`, `GetUpvalue`, `SetUpvalue`, `IterGuardEnd`, `FillSelfCapture`) — o estudo acima diz **como** devem evoluir, não que estão ausentes.

## 3. Propostas que continuam propostas

**Janelas de registradores:** guardar base, limite e return PC por ativação, ampliar armazenamento por slots e fechar capturas antes de invalidar frames. Exige semântica de unwind e bench de chamadas, não apenas substituir um array por um Vec.

**Shapes/slots/atoms:** separar identidade de tipo, nomes internados e posição de fields; manter fallback para layouts diferentes e medir acertos/falhas do cache. Não transformar structs em objetos com a semântica de JS.

**Closures e async Reg:** implementar células compartilhadas, self-capture, await e scheduler conforme o runtime existente. Rejeitar programas sem suporte permanece melhor que omitir instruções.

**Versionamento estrutural:** cada operação de adição/remoção incrementa uma versão; IterGuard registra versão e identidade. Isso fecha a lacuna de alterações que preservam comprimento, mas exige alterar todos os mutadores e a política de callbacks.

**Host/sandbox:** orçamento de bytecode e fuel não limitam callbacks bloqueantes, capacidade de filesystem/processos, memória nem saída. Reutilizar o contrato AHS e capabilities da Aipo; não presumir que copiar globais da Stack VM instala seus serviços no RegVM.

**Alternativas Wasm:** comparar wasmi, WAMR e outros candidatos com os mesmos módulos, imports e checksums. O núcleo wasmi no_std e seu adaptador WASI/std são caminhos distintos. Seleção exige medições e decisão documentada.

**Perfis shell/nano:** auditar grafo por features e medir startup/tamanho reais; o perfil Cargo atual não entrega no_std. Manter os perfis no mesmo workspace e preservar uma semântica.

## 4. Piso de dispatch (candidatos)

- Alvo de trabalho: **~100 ns/opcode** (estimativa de ordem de grandeza para calibrar benchmarks, **não** medida neste estudo).
- Escada verificada nos fontes: `switch` (Lua, `lvm.c:1199-1204`) → `goto` computado com fallback (Wren, `wren_vm.c:890-918`; Janet, `vm.c:54-68`) → assembly gerado (LuaJIT, `vm_*.dasc`).
- Recomendação: RegVm começa no `switch`, adiciona `goto` computado por feature/flag como Wren, mede; DynASM só com evidência do profiler.

## 5. P2 — matriz de perfis (nano/shell/full)

| Perfil | Agora | Depois | Regra de desenho |
|---|---|---|---|
| `nano` | Linux pequeno+rápido | MCU | `no_std` desde o início; trio de representação como Janet (`janet.h:540-577`) |
| `shell` | CLI interativo | — | `goto` computado opcional; GC por limiar (QuickJS, `quickjs.c:1687-1696`) |
| `full` | wasmtime permitido | — | Otimizações agressivas isoladas por feature |

## 6. P3 — substrato wasm

| Candidato | Estado neste estudo |
|---|---|
| `wasmtime` | Full-only (~25 MB); nunca no nano/shell |
| `WAMR` | **Candidato de experimento**: WASI + `RISCV32` + flags (`FAST_INTERP`, `MINI_LOADER`) — `reference-runtimes.md §4` |
| `wasm3` | Menor (~64 KB / 10 KB RAM) + perfil determinístico como modelo de replay |
| `wasmi` | Alternativa Rust-nativa com `wasmi_wasi` (`wasip1`); decidir no spike |

Spike P3 mede os três com WASI ligado antes de qualquer ADP (critérios: KB, `RISCV32`, determinismo, `no_std`, metering).

## 7. P4 — regras anti-fragmentação

1. **Uma semântica / N emissores**: estudos nunca criam dialeto; só ADP muda semântica.
2. **Tier 1/2/3 com erros amigáveis**: Tier 1 = núcleo; Tier 2/3 = recusas com mensagem acionável (modelo Luau `Error.h`), nunca silencioso.
3. **Corpus de conformance como árbitro**: divergência entre perfis = bug no corpus, não "comportamento de perfil".
4. **Perfis via `features` + `[profile.*]`, nunca crates novas**: nano/shell/full são combinações de flags, não forks.
5. **Matriz CI `full/shell/nano`**: todo PR roda os três perfis; falha num perfil bloqueia merge.

## 8. Ordem de continuidade

1. Executar, em rodada autorizada futura, conformance e os gates desta reconstrução.
2. Verificador integral Reg, versionamento de coleções e políticas de host/output.
3. Paridade de métodos, closures, async, hooks e contratos estruturais.
4. Janelas/slots e caches com benchmark pareado, sem transportar resultados de upstream.
5. Perfil nano/shell e spike de engine alternativa com decisão de licença e portabilidade.

O [guia de uso e reimplementação](runtime-hardening-guide.md) contém APIs, mudanças de compatibilidade e passos de cada correção. O [guia master](llm-study-guide.md) mantém a metodologia e os critérios de aceite que ainda não foram certificados.
