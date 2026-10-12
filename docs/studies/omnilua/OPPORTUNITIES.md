# Matriz de Oportunidades: Reaproveitamento omniLua → Aipo-lang

- **Status da Auditoria:** Fase 0 (Classificação Inicial)
- **Fórmula de Ordenação:**  
  $$\text{Pontuação} = 3B + 3F + 2R + 2M - 2C - 3G - 2U$$  
  *(Escala de 0 a 5 por critério; maior pontuação = maior viabilidade de retorno sobre risco)*

---

## 1. Tabela Geral de Oportunidades Auditadas

| ID | Oportunidade / Técnica | Via Proposta | Prioridade | $B$ | $F$ | $R$ | $M$ | $C$ | $G$ | $U$ | Score | Decisão Preliminar |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **AIPO-OMNI-001** | **Canários de Vazamento de Memória (`CountingAllocator`)** | `ADAPT` | **P0** | 4 | 5 | 5 | 5 | 1 | 1 | 1 | **+38** | **CONCLUÍDO & CERTIFICADO** ([Doc](opportunities/AIPO-OMNI-001.md)) |
| **AIPO-OMNI-002** | **Sandboxing: Instruction Budget Ininterceptável (`Fault` Sticky)** | `ADAPT` | **P0** | 5 | 5 | 5 | 4 | 2 | 1 | 1 | **+37** | **CONCLUÍDO & CERTIFICADO** ([Doc](opportunities/AIPO-OMNI-002.md)) |
| **AIPO-OMNI-003** | **API de Embedding Rust Scoped (`AipoEngine::scope`)** | `ADAPT` | **P0** | 5 | 5 | 4 | 4 | 3 | 2 | 2 | **+28** | **CONCLUÍDO & CERTIFICADO** ([Doc](opportunities/AIPO-OMNI-003.md)) |
| **AIPO-OMNI-004** | **Tarifação Periódica de Fuel no Despacho (Interval Metering)** | `ADAPT` | **P1** | 4 | 5 | 3 | 3 | 2 | 2 | 1 | **+27** | **CONCLUÍDO & CERTIFICADO** ([Doc](opportunities/AIPO-OMNI-004.md)) |
| **AIPO-OMNI-005** | **Janelas de Registrador na RegVm (Substituir cópia de 256 regs)** | `ADAPT` | **P1** | 5 | 4 | 2 | 3 | 4 | 3 | 2 | **+18** | **CONCLUÍDO & CERTIFICADO** ([Doc](opportunities/AIPO-OMNI-005.md)) |
| **AIPO-OMNI-006** | **Tracing Garbage Collector (Substituir modelo `Rc`/arenas)** | `DEFER` | **P2** | 3 | 2 | 2 | 1 | 5 | 5 | 4 | **-14** | **ADIADO (Exige Profiling)** |
| **AIPO-OMNI-007** | **Byte Strings Arbitrárias como Representação de Texto** | `REJECT` | — | 1 | 0 | 0 | 0 | 5 | 5 | 5 | **-32** | **REJEITADO (Quebra UTF-8)** |
| **AIPO-OMNI-008** | **Tabelas Híbridas / Metatables Dinâmicas de Lua** | `REJECT` | — | 1 | 0 | 0 | 0 | 5 | 5 | 5 | **-32** | **REJEITADO (Quebra Tipagem)** |
| **AIPO-OMNI-009** | **Gramática / Delimitadores de Bloco (`do...end`)** | `REJECT` | — | 0 | 0 | 0 | 0 | 5 | 5 | 5 | **-35** | **REJEITADO (ADR-001)** |

---

## 2. Detalhamento dos Três Candidatos P0 Aprovados para Prova de Conceito

### Candidato P0-1: AIPO-OMNI-001 — Canários de Vazamento de Memória (`CountingAllocator`)
- **Problema na Aipo:** O modelo de objetos utiliza `Rc<RefCell<...>>` para structs, listas, dicionários e closures. Embora o `CycleGuard` previna a criação de ciclos de referência óbvios em atribuições mutáveis, não há teste de integração garantindo que criar 1.000 instâncias de `Vm`, rodar scripts com closures aninhados e destruí-las resulte em saldo zero de bytes no heap.
- **Referência no omniLua:** `crates/lua-rs-runtime/tests/leak_canaries.rs` e `harness/strict_guard_check.sh`.
- **Implementação Proposta:** Criar um teste em `crates/aipo-testkit` configurando um alocador global de contagem durante os testes, verificando que o ciclo de vida completo de VMs, tarefas e módulos descarta 100% da memória alocada.
- **Métrica de Sucesso:** $\Delta \text{bytes} = 0$ após ciclo de vida de churn em 100 iterações.

### Candidato P0-2: AIPO-OMNI-002 — Instruction Budget Ininterceptável (`Fault` Sticky)
- **Problema na Aipo:** Scripts de plugins em jogos (`aipo-game-host`) ou ferramentas CLI não devem conseguir travar o processo com `while true { }`. A Aipo já possui `VmBudget`, mas se o usuário encapsular um loop infinito dentro de um bloco `attempt { ... }`, um tratamento ingênuo de erro poderia engolir a falha de fuel e continuar o loop para sempre.
- **Referência no omniLua:** `crates/lua-vm/src/state.rs:SandboxLimits` e `docs/SANDBOXING_EXPLORATION.md`.
- **Implementação Proposta:** Sinalizar o esgotamento de fuel através de um `VmFault::BudgetExhausted` ininterceptável. Quando emitido, o despachante desbobina a pilha incondicionalmente, ignorando qualquer handler de `attempt` ativo até o retorno ao host.
- **Métrica de Sucesso:** Script `attempt { while true { } }` aborta em menos de 10ms com saída limpa no host, sem congelar a thread.

### Candidato P0-3: AIPO-OMNI-003 — API de Embedding Scoped (`AipoEngine::scope`)
- **Problema na Aipo:** Integrar o host Rust a scripts Aipo (ex: no motor Miniquad ou em plugins) hoje depende de registros globais ou da C ABI. Não existe uma API segura em Rust puro que permita emprestar uma referência mutável do host (`&mut GameWorld`) pelo tempo exato de execução de um callback ou script, com garantia estática e dinâmica de que a referência não escapa.
- **Referência no omniLua:** `Lua::scope` e `create_userdata_ref_mut` em `crates/lua-rs-runtime/src/lib.rs:50-86`.
- **Implementação Proposta:** Construir uma fachada de alto nível em Rust puro conectada ao `HandleTable` geracional de `aipo-host`, invalidando imediatamente os handles ao término do escopo. Tentativas do script de guardar a referência em variáveis globais disparam `AIPO_RT_SCOPE_ESCAPE` ou erro de stale handle no próximo frame.
- **Métrica de Sucesso:** Zero `unsafe` adicionado; cobertura de testes provando falha segura se um script tentar acessar o handle fora do bloco `scope`.
